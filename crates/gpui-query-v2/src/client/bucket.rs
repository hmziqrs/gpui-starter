//! Type-partitioned bucket for query resources.
//!
//! **v2 improvements**:
//! - Uses `AHashMap` instead of `std::collections::HashMap`
//! - Co-locates `RequestSequencer` with entity in `BucketEntry`
//! - Collect-then-update pattern avoids nested entity borrows
//!
//! **Audit fixes (findings 1-5)**:
//! - Uses `WeakEntity` to avoid preventing GC of unused resources (finding 3)
//! - Tracks observer count in `BucketEntry` to protect in-use resources (finding 4)
//! - Enforces minimum GC time of 1000ms to avoid aggressive eviction (finding 1)
//! - Implements actual policy updates and calls them from `get_or_create` (findings 2, 5)
//!
//! **Audit 3 fixes**:
//! - Caches `status_snapshot` and `last_updated_ms` in `BucketEntry` so GC can
//!   filter without acquiring entity read locks (finding 1)
//! - Uses `HashMap::retain()` instead of collect-then-remove to avoid
//!   intermediate `Vec<QueryKey>` allocation (finding 2)
//! - `invalidate_matching`/`reset_matching` collect cheap keys then upgrade
//!   individually, deferring the `upgrade()` cost (finding 3)
//! - Configurable max entry count per bucket to prevent unbounded growth (finding 4)
//! - GC protects `StaleWhileRevalidate` resources within the stale window
//!   from premature eviction (finding 5)
//! - GC evicts `Success` resources whose age exceeds `2 * gc_time_ms` (finding 6)
//! - `retain()`/`release()` doc comments note wiring requirement (finding 7)

use ahash::AHashMap;
use gpui::{App, AppContext as _, WeakEntity};

use crate::core::{
    CachePolicy, QueryKey, QueryKeyFilter, QueryResource, QueryStatus, RequestPolicy,
    RequestSequencer,
};

use super::ErasedBucket;
use super::devtools::QueryDiagnostic;

/// Minimum GC time in milliseconds.
///
/// A `gc_time_ms` of 0 would cause every `Idle` and `Failure` resource to be
/// evicted on every GC pass (since `age_ms >= 0` is always true for unsigned
/// values). This effectively disables caching for non-active resources.
/// Enforcing a 1-second minimum prevents this footgun.
const MIN_GC_TIME_MS: u64 = 1_000;

/// Default maximum number of entries per bucket.
///
/// Prevents unbounded memory growth from malicious or buggy components that
/// register unlimited unique query keys. When the limit is reached, the
/// oldest (least-recently-updated) entry is evicted to make room.
const DEFAULT_MAX_ENTRIES: usize = 10_000;

/// Multiplier applied to `gc_time_ms` to determine the maximum age for
/// `Success` resources before they become eligible for eviction.
///
/// A `Success` resource with zero observers whose data age exceeds
/// `SUCCESS_GC_MULTIPLIER * gc_time_ms` will be evicted even though it
/// holds valuable data. This prevents memory leaks from queries that
/// succeeded once but were never observed again.
const SUCCESS_GC_MULTIPLIER: u32 = 2;

/// Cached status snapshot for GC decisions, updated on each request completion.
///
/// This allows the GC to filter entries without acquiring entity read locks
/// (finding 1 fix). The trade-off is a small per-update cost: when a request
/// completes (success or failure), the hook layer calls
/// `update_status_snapshot` to sync this cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusSnapshot {
    /// The `QueryStatus` at the time of the last snapshot.
    pub status: QueryStatus,
    /// `last_updated_at` in milliseconds since UNIX epoch, or `None` if the
    /// resource has never completed a fetch.
    pub last_updated_ms: Option<u128>,
    /// The `CachePolicy` at the time of the last snapshot.
    pub cache_policy: CachePolicy,
}

impl Default for StatusSnapshot {
    fn default() -> Self {
        Self {
            status: QueryStatus::Idle,
            last_updated_ms: None,
            cache_policy: CachePolicy::default(),
        }
    }
}

/// Entry co-locating weak entity reference, sequencer, observer tracking,
/// and cached status for GC.
///
/// Uses `WeakEntity` instead of `Entity` so that the bucket does not prevent
/// GPUI from garbage-collecting unused query resources. The weak reference is
/// upgraded on access; if the entity was already collected, the entry is
/// treated as missing and re-created on next `get_or_create`.
///
/// The `observer_count` field tracks the number of active `QueryObserver`
/// subscriptions held by mounted components. GC will refuse to evict entries
/// with `observer_count > 0`, preserving cache deduplication for in-use
/// resources regardless of their state (including `Success`).
///
/// The `status_snapshot` field caches the resource's status and
/// `last_updated_at` so GC can make eviction decisions without acquiring
/// entity read locks (finding 1 fix). It is updated via
/// `update_status_snapshot` after each request completion.
struct BucketEntry<T, E> {
    entity: WeakEntity<QueryResource<T, E>>,
    sequencer: RequestSequencer,
    /// Number of active observer subscriptions for this resource.
    /// Incremented when an observer is attached, decremented when dropped.
    ///
    /// **Note (finding 7)**: The hook layer (`use_query`, `use_query_manual`)
    /// must call `bucket.retain()` after creating an observer and
    /// `bucket.release()` when the subscription is dropped. Without these
    /// calls, `observer_count` remains 0 and GC protection for observed
    /// resources is ineffective.
    observer_count: usize,
    /// Cached status for GC decisions, updated on each request completion.
    /// Allows GC to filter entries without acquiring entity read locks.
    status_snapshot: StatusSnapshot,
}

/// Type-partitioned storage for query resources of a specific `(T, E)` type pair.
pub struct QueryBucket<T, E> {
    entries: AHashMap<QueryKey, BucketEntry<T, E>>,
    /// Maximum number of entries allowed in this bucket.
    /// When exceeded, the oldest entry (by `last_updated_ms`) is evicted.
    max_entries: usize,
}

impl<T: Clone + Send + Sync + 'static, E: Clone + Send + Sync + 'static> QueryBucket<T, E> {
    /// Create a new bucket with the default max entry limit.
    pub(crate) fn new() -> Self {
        Self {
            entries: AHashMap::new(),
            max_entries: DEFAULT_MAX_ENTRIES,
        }
    }

    /// Create a new bucket with a custom max entry limit.
    pub(crate) fn with_max_entries(max_entries: usize) -> Self {
        Self {
            entries: AHashMap::new(),
            max_entries: max_entries.max(1),
        }
    }

    /// Evict the oldest (least-recently-updated) entry to make room for a new one.
    ///
    /// Called when `get_or_create` would exceed `max_entries`. Prefers evicting
    /// entries with zero observers and the oldest `last_updated_ms`. If all
    /// entries have observers, evicts the oldest observed entry as a last resort.
    fn evict_oldest(&mut self) {
        let mut oldest_key: Option<QueryKey> = None;
        let mut oldest_age: u128 = u128::MAX;
        let mut found_unobserved = false;

        for (key, entry) in &self.entries {
            let is_unobserved = entry.observer_count == 0;

            // If we've already found an unobserved entry, skip observed ones.
            if found_unobserved && !is_unobserved {
                continue;
            }

            let age = entry.status_snapshot.last_updated_ms.unwrap_or(0);
            if age < oldest_age {
                oldest_key = Some(key.clone());
                oldest_age = age;
                found_unobserved = found_unobserved || is_unobserved;
            }
        }

        if let Some(key) = oldest_key {
            self.entries.remove(&key);
        }
    }

    /// Get an existing entity or create a new one.
    ///
    /// If the key already exists and the weak reference can be upgraded, the
    /// existing entity is returned. If the entity was already collected (all
    /// strong references dropped), the stale entry is replaced with a fresh one.
    ///
    /// When the key already exists and the policies differ from the stored
    /// resource's current policies, the resource is updated in-place via
    /// `set_cache_policy` / `set_request_policy`.
    ///
    /// When creating a new entry would exceed `max_entries`, the oldest entry
    /// is evicted first (finding 4 fix).
    pub(crate) fn get_or_create(
        &mut self,
        key: QueryKey,
        cache_policy: CachePolicy,
        request_policy: RequestPolicy,
        cx: &mut App,
    ) -> gpui::Entity<QueryResource<T, E>> {
        if let Some(entry) = self.entries.get(&key) {
            if let Some(entity) = entry.entity.upgrade() {
                // Audit fix (findings 2, 5): Update policies if they differ.
                let needs_update = entity.read_with(cx, |resource, _| {
                    resource.cache_policy() != cache_policy
                        || resource.request_policy() != request_policy
                });
                if needs_update {
                    entity.update(cx, |resource, _| {
                        resource.set_cache_policy(cache_policy);
                        resource.set_request_policy(request_policy);
                    });
                    // Update the cached cache_policy in the snapshot.
                    if let Some(entry) = self.entries.get_mut(&key) {
                        entry.status_snapshot.cache_policy = cache_policy;
                    }
                }
                return entity;
            }
            // Weak reference is dead — remove the stale entry so we can re-create.
            self.entries.remove(&key);
        }

        // Enforce max entries limit (finding 4 fix).
        if self.entries.len() >= self.max_entries {
            self.evict_oldest();
        }

        let entity = cx.new(|_| QueryResource::new(key.clone(), cache_policy, request_policy));
        self.entries.insert(
            key,
            BucketEntry {
                entity: entity.downgrade(),
                sequencer: RequestSequencer::new(),
                observer_count: 0,
                status_snapshot: StatusSnapshot {
                    status: QueryStatus::Idle,
                    last_updated_ms: None,
                    cache_policy,
                },
            },
        );
        entity
    }

    /// Get an existing entity by key.
    ///
    /// Returns `None` if the key is not in the bucket or if the weak reference
    /// can no longer be upgraded (the entity was collected).
    pub(crate) fn get(&self, key: &QueryKey) -> Option<gpui::Entity<QueryResource<T, E>>> {
        self.entries.get(key).and_then(|e| e.entity.upgrade())
    }

    /// All entities in this bucket that are still alive.
    pub(crate) fn all_entities(&self) -> Vec<gpui::Entity<QueryResource<T, E>>> {
        self.entries
            .values()
            .filter_map(|e| e.entity.upgrade())
            .collect()
    }

    /// Update policies for an existing entry.
    ///
    /// Sets the cache and request policies on the resource if the key is found
    /// and the entity is still alive. No-op if the key is not found or the
    /// entity has been collected.
    pub(crate) fn update_policies(
        &mut self,
        key: &QueryKey,
        cache_policy: CachePolicy,
        request_policy: RequestPolicy,
        cx: &mut App,
    ) {
        if let Some(entry) = self.entries.get(key) {
            if let Some(entity) = entry.entity.upgrade() {
                entity.update(cx, |resource, _| {
                    resource.set_cache_policy(cache_policy);
                    resource.set_request_policy(request_policy);
                });
            }
        }
    }

    /// Increment the observer count for an entry.
    ///
    /// Call this when a `QueryObserver` subscription is attached to the
    /// resource. This prevents GC from evicting the entry while components
    /// are actively observing it.
    ///
    /// **Wiring requirement (finding 7)**: The hook layer must call this
    /// after creating an observer. Without it, `observer_count` is always 0.
    pub(crate) fn retain(&mut self, key: &QueryKey) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.observer_count = entry.observer_count.saturating_add(1);
        }
    }

    /// Decrement the observer count for an entry.
    ///
    /// Call this when a `QueryObserver` subscription is dropped (e.g., the
    /// component unmounted).
    ///
    /// **Wiring requirement (finding 7)**: The hook layer must call this
    /// when the subscription is dropped. Implement an `ObserverGuard` or
    /// custom `Drop` guard that wraps the subscription and calls `release()`.
    pub(crate) fn release(&mut self, key: &QueryKey) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.observer_count = entry.observer_count.saturating_sub(1);
        }
    }

    /// Get a mutable reference to the sequencer for an entry.
    ///
    /// Returns `None` if the key is not found.
    pub(crate) fn sequencer_mut(&mut self, key: &QueryKey) -> Option<&mut RequestSequencer> {
        self.entries.get_mut(key).map(|e| &mut e.sequencer)
    }

    /// Update the cached status snapshot for an entry.
    ///
    /// Call this after each request completion (success or failure) so the
    /// GC can make eviction decisions without acquiring entity read locks.
    /// This is the fix for finding 1 (O(n * m) GC with entity read locks).
    pub(crate) fn update_status_snapshot(
        &mut self,
        key: &QueryKey,
        status: QueryStatus,
        last_updated_ms: Option<u128>,
        cache_policy: CachePolicy,
    ) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.status_snapshot = StatusSnapshot {
                status,
                last_updated_ms,
                cache_policy,
            };
        }
    }
}

impl<T: Clone + Send + Sync + 'static, E: Clone + Send + Sync + 'static> ErasedBucket
    for QueryBucket<T, E>
{
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    /// Garbage-collect stale query resources.
    ///
    /// Evicts entries where:
    /// 1. The entry has no active observers (`observer_count == 0`), AND
    /// 2. One of the following is true:
    ///    a. The weak reference is dead (entity was collected), OR
    ///    b. The resource is in an evictable state (`Idle` or `Failure`) and
    ///       data age exceeds `gc_time_ms`, OR
    ///    c. The resource is in `Success` state and data age exceeds
    ///       `SUCCESS_GC_MULTIPLIER * gc_time_ms` (finding 6 fix), OR
    ///    d. The resource is `Success` with a `StaleWhileRevalidate` policy
    ///       and data age exceeds the total valid window (finding 5 fix).
    ///
    /// Resources that are actively loading are always retained. Resources with
    /// active observer subscriptions are always retained.
    ///
    /// Uses `HashMap::retain()` to avoid the intermediate `Vec<QueryKey>`
    /// allocation (finding 2 fix). Uses cached `StatusSnapshot` to avoid
    /// acquiring entity read locks during iteration (finding 1 fix).
    fn gc(&mut self, now_ms: u128, gc_time_ms: u64, _cx: &App) {
        // Audit fix (finding 1): Enforce minimum GC time.
        // gc_time_ms == 0 would cause age_ms >= 0 to be always true (u128),
        // evicting every Idle/Failure resource immediately.
        let gc_time_ms = gc_time_ms.max(MIN_GC_TIME_MS);
        let gc_threshold = gc_time_ms as u128;

        // Finding 6: Success resources are evicted at 2x the gc_time threshold.
        let success_threshold = gc_threshold * (SUCCESS_GC_MULTIPLIER as u128);

        // Finding 2 fix: Use retain() instead of collect-then-remove to avoid
        // allocating an intermediate Vec<QueryKey>.
        // Finding 1 fix: Use cached StatusSnapshot instead of entity.read(cx)
        // to avoid acquiring read locks during iteration.
        self.entries.retain(|_key, entry| {
            // Clean up entries whose entity has already been collected.
            // We still upgrade to check liveness, but do NOT call entity.read(cx).
            if entry.entity.upgrade().is_none() {
                return false; // Dead reference — evict.
            }

            // Audit fix (finding 4): Never evict entries with active observers.
            // Resources with observer_count > 0 have mounted components that
            // hold Entity references. Evicting would break cache deduplication:
            // the next `get_or_create` would produce a new resource instead of
            // returning the existing one.
            if entry.observer_count > 0 {
                return true; // Keep — has active observers.
            }

            let snapshot = &entry.status_snapshot;

            // Never evict resources that are actively loading.
            if snapshot.status.is_loading() {
                return true;
            }

            // Compute age from the cached snapshot (no entity read lock needed).
            let age_ms = snapshot
                .last_updated_ms
                .map(|updated| now_ms.saturating_sub(updated))
                .unwrap_or(gc_threshold); // No timestamp → treat as expired.

            // Finding 5 fix: Protect StaleWhileRevalidate resources that are
            // within the stale window (candidates for background revalidation).
            // A Success resource with SWR policy whose data is within
            // ttl_ms + stale_ms should not be evicted — it may be served as
            // stale data while a background refetch is triggered.
            if snapshot.status == QueryStatus::Success {
                let cache_policy = snapshot.cache_policy;
                if cache_policy.can_serve_stale() {
                    // Data within the total valid window (ttl + stale) is
                    // still a candidate for background revalidation — protect it.
                    if !cache_policy.is_expired(age_ms) {
                        return true;
                    }
                    // Data is past the total valid window. Fall through to
                    // the Success age check below.
                }

                // Finding 6 fix: Evict Success resources whose data age
                // exceeds SUCCESS_GC_MULTIPLIER * gc_time_ms. This prevents
                // memory leaks from queries that succeeded once but were never
                // observed again.
                if age_ms < success_threshold {
                    return true; // Still within the Success retention window.
                }
                // Success resource is very old — evict it.
                return false;
            }

            // Only evict if in a terminal, non-success state.
            let evictable = matches!(
                snapshot.status,
                QueryStatus::Idle | QueryStatus::Failure
            );
            if !evictable {
                return true; // Keep — not evictable (e.g. Cancelled).
            }

            // Evict Idle/Failure resources whose data age exceeds gc_time_ms.
            age_ms < gc_threshold
        });
    }

    fn count(&self) -> usize {
        self.entries.len()
    }

    /// **v2 fix + finding 3 fix**: Collect keys (cheap Arc increments) then
    /// upgrade individually, deferring the `upgrade()` cost and avoiding
    /// upgrades for entities that may have been collected between collection
    /// and update.
    fn invalidate_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App) {
        // Finding 3 fix: Collect just the keys (cheap Arc increments) rather
        // than upgrading all weak references upfront. Then iterate keys,
        // upgrade each individually, and update. This defers the upgrade()
        // cost and avoids upgrading entities that may have been collected
        // between collection and update.
        let keys: Vec<QueryKey> = self
            .entries
            .keys()
            .filter(|key| filter.matches(key))
            .cloned()
            .collect();

        for key in keys {
            if let Some(entry) = self.entries.get(&key) {
                if let Some(entity) = entry.entity.upgrade() {
                    entity.update(cx, |resource, _| {
                        resource.invalidate();
                    });
                }
            }
        }
    }

    fn reset_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App) {
        // Finding 3 fix: Same pattern as invalidate_matching — collect keys
        // first, upgrade individually.
        let keys: Vec<QueryKey> = self
            .entries
            .keys()
            .filter(|key| filter.matches(key))
            .cloned()
            .collect();

        for key in keys {
            if let Some(entry) = self.entries.get(&key) {
                if let Some(entity) = entry.entity.upgrade() {
                    entity.update(cx, |resource, _| {
                        resource.reset();
                    });
                    // Update snapshot after reset — status goes to Idle.
                    if let Some(entry) = self.entries.get_mut(&key) {
                        entry.status_snapshot.status = QueryStatus::Idle;
                        entry.status_snapshot.last_updated_ms = None;
                    }
                }
            }
        }
    }

    fn remove_matching(&mut self, filter: &QueryKeyFilter) {
        self.entries.retain(|k, _| !filter.matches(k));
    }

    /// Cancel in-flight requests for entries matching the filter.
    ///
    /// Uses the collect-keys-then-update pattern to avoid mutating the
    /// `HashMap` during iteration. Only cancels entries that have an
    /// active request (status is loading).
    ///
    /// Since the type parameter `E` is not known at the erased trait level,
    /// cancellation is performed by cancelling the cooperative signal and
    /// resetting the resource. The in-flight fetcher will observe the
    /// cancelled signal and should abort. The resource's active_request_id
    /// is cleared so that when the fetcher completes, the stale result is
    /// discarded by `accept_current_request`.
    fn cancel_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App) {
        let keys: Vec<QueryKey> = self
            .entries
            .keys()
            .filter(|key| filter.matches(key))
            .cloned()
            .collect();

        for key in keys {
            if let Some(entry) = self.entries.get(&key) {
                if let Some(entity) = entry.entity.upgrade() {
                    // Only cancel if the resource has an active request.
                    let is_loading = entity.read_with(cx, |r, _| r.is_loading());
                    if is_loading {
                        entity.update(cx, |resource, _| {
                            // Cancel the cooperative signal so the fetcher
                            // can observe it and abort.
                            if let Some(signal) = resource.signal() {
                                signal.cancel();
                            }
                            // Clear the active request ID so any in-flight
                            // completion is rejected as stale.
                            resource.mark_ignored_result();
                        });
                        // Update snapshot after cancel.
                        if let Some(entry) = self.entries.get_mut(&key) {
                            entry.status_snapshot.status = QueryStatus::Cancelled;
                        }
                    }
                }
            }
        }
    }

    /// Collect per-resource diagnostic details for all live entries.
    ///
    /// Iterates all bucket entries, upgrades weak references, reads entity
    /// state, and constructs a `QueryDiagnostic` for each live resource.
    /// Dead entries (collected entities) are skipped.
    fn collect_diagnostics(&self, now_ms: u128, cx: &App) -> Vec<QueryDiagnostic> {
        self.entries
            .iter()
            .filter_map(|(key, entry)| {
                let entity = entry.entity.upgrade()?;
                let resource = entity.read(cx);
                Some(QueryDiagnostic {
                    key: key.to_path(),
                    status: resource.status(),
                    cache_policy: resource.cache_policy().label(),
                    cache_age_ms: resource.cache_age_ms(now_ms),
                    cache_hits: resource.cache_hits(),
                    retry_count: resource.retry_count(),
                })
            })
            .collect()
    }
}
