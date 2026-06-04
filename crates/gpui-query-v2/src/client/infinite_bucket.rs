//! Type-partitioned bucket for infinite query resources.
//!
//! Mirrors [`QueryBucket`] but for [`InfiniteQueryResource`]. Co-locates a
//! [`RequestSequencer`] with each entity so that request IDs are monotonic
//! across the lifetime of the resource (audit fix: persistent sequencer).
//!
//! **Audit fixes**:
//! - Uses `WeakEntity` to avoid preventing GC of unused resources
//! - Tracks observer count to protect in-use resources from eviction
//! - Enforces minimum GC time of 1000ms to avoid aggressive eviction

use ahash::AHashMap;
use gpui::{App, AppContext as _, WeakEntity};

use super::bucket::StatusSnapshot;
use crate::core::{
    CachePolicy, InfiniteQueryResource, QueryKey, QueryKeyFilter, QueryStatus, RequestPolicy,
    RequestSequencer,
};

/// Minimum GC time in milliseconds (mirrors `bucket::MIN_GC_TIME_MS`).
const MIN_GC_TIME_MS: u64 = 1_000;

/// Entry co-locating weak entity reference, sequencer, and observer tracking.
struct InfiniteBucketEntry<T, E> {
    entity: WeakEntity<InfiniteQueryResource<T, E>>,
    sequencer: RequestSequencer,
    /// Number of active observer subscriptions for this resource.
    observer_count: usize,
    /// Cached status for GC decisions, updated on each request completion.
    ///
    /// Perf: allows GC to filter entries without acquiring entity read locks
    /// during the iteration loop. Mirrors the same pattern in `BucketEntry`.
    status_snapshot: StatusSnapshot,
}

/// Type-partitioned storage for infinite query resources of a specific `(T, E)` type pair.
pub struct InfiniteQueryBucket<T, E> {
    entries: AHashMap<QueryKey, InfiniteBucketEntry<T, E>>,
}

impl<T: Clone + Send + Sync + 'static, E: Clone + Send + Sync + 'static> InfiniteQueryBucket<T, E> {
    pub(crate) fn new() -> Self {
        Self {
            entries: AHashMap::new(),
        }
    }

    /// Get an existing entity or create a new one.
    ///
    /// If the key already exists and the weak reference can be upgraded, returns
    /// the existing entity. If the entity was collected, the stale entry is replaced.
    ///
    /// When the key already exists and the policies differ, updates in-place.
    pub(crate) fn get_or_create(
        &mut self,
        key: QueryKey,
        cache_policy: CachePolicy,
        request_policy: RequestPolicy,
        cx: &mut App,
    ) -> gpui::Entity<InfiniteQueryResource<T, E>> {
        if let Some(entry) = self.entries.get(&key) {
            if let Some(entity) = entry.entity.upgrade() {
                // Update policies if they differ.
                let needs_update = entity.read_with(cx, |resource, _| {
                    resource.cache_policy() != cache_policy
                        || resource.request_policy() != request_policy
                });
                if needs_update {
                    entity.update(cx, |resource, _| {
                        resource.set_cache_policy(cache_policy);
                        resource.set_request_policy(request_policy);
                    });
                }
                return entity;
            }
            // Weak reference is dead — remove the stale entry.
            self.entries.remove(&key);
        }

        let entity = cx.new(|_| InfiniteQueryResource::new(key.clone(), cache_policy, request_policy));
        self.entries.insert(
            key,
            InfiniteBucketEntry {
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
    pub(crate) fn get(&self, key: &QueryKey) -> Option<gpui::Entity<InfiniteQueryResource<T, E>>> {
        self.entries.get(key).and_then(|e| e.entity.upgrade())
    }

    /// Get the sequencer for a given key, if it exists.
    #[allow(dead_code)]
    pub(crate) fn sequencer(&self, key: &QueryKey) -> Option<&RequestSequencer> {
        self.entries.get(key).map(|e| &e.sequencer)
    }

    /// Get a mutable reference to the sequencer for an entry.
    ///
    /// Returns `None` if the key is not found.
    pub(crate) fn sequencer_mut(&mut self, key: &QueryKey) -> Option<&mut RequestSequencer> {
        self.entries.get_mut(key).map(|e| &mut e.sequencer)
    }

    /// All entities in this bucket that are still alive.
    pub(crate) fn all_entities(&self) -> Vec<gpui::Entity<InfiniteQueryResource<T, E>>> {
        self.entries
            .values()
            .filter_map(|e| e.entity.upgrade())
            .collect()
    }

    /// Increment the observer count for an entry.
    #[allow(dead_code)]
    pub(crate) fn retain(&mut self, key: &QueryKey) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.observer_count = entry.observer_count.saturating_add(1);
        }
    }

    /// Decrement the observer count for an entry.
    #[allow(dead_code)]
    pub(crate) fn release(&mut self, key: &QueryKey) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.observer_count = entry.observer_count.saturating_sub(1);
        }
    }

    /// Update the cached status snapshot for an entry.
    ///
    /// Call this after each request completion (success or failure) so the
    /// GC can make eviction decisions without acquiring entity read locks.
    /// Mirrors `QueryBucket::update_status_snapshot`.
    #[allow(dead_code)]
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

// Implement the erased trait so InfiniteQueryBucket can live in QueryClient's
// heterogeneous map.
use super::ErasedInfiniteBucket;
use super::devtools::QueryDiagnostic;

impl<T: Clone + Send + Sync + 'static, E: Clone + Send + Sync + 'static> ErasedInfiniteBucket
    for InfiniteQueryBucket<T, E>
{
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    /// Garbage-collect stale infinite query resources.
    ///
    /// Uses cached `StatusSnapshot` to avoid acquiring entity read locks
    /// during iteration (same pattern as `QueryBucket::gc`). The snapshot is
    /// updated via `update_status_snapshot` after each request completion.
    ///
    /// Uses `HashMap::retain()` to avoid the intermediate `Vec<QueryKey>`
    /// allocation.
    fn gc(&mut self, now_ms: u128, gc_time_ms: u64, _cx: &App) {
        let gc_time_ms = gc_time_ms.max(MIN_GC_TIME_MS);
        let gc_threshold = gc_time_ms as u128;

        self.entries.retain(|_key, entry| {
            // Clean up entries whose entity has already been collected.
            // We still upgrade to check liveness, but do NOT call entity.read(cx).
            if entry.entity.upgrade().is_none() {
                return false; // Dead reference — evict.
            }

            // Never evict entries with active observers.
            if entry.observer_count > 0 {
                return true;
            }

            let snapshot = &entry.status_snapshot;

            // Never evict resources that are actively loading.
            if snapshot.status.is_loading() {
                return true;
            }

            // Only evict if in a terminal, non-success state.
            let evictable = matches!(
                snapshot.status,
                QueryStatus::Idle | QueryStatus::Failure
            );
            if !evictable {
                return true; // Keep — not evictable (e.g. Success, Cancelled).
            }

            // Check cache age from the cached snapshot (no entity read lock needed).
            let age_ms = snapshot
                .last_updated_ms
                .map(|updated| now_ms.saturating_sub(updated))
                .unwrap_or(gc_threshold); // No timestamp → treat as expired.
            age_ms < gc_threshold
        });
    }

    fn count(&self) -> usize {
        self.entries.len()
    }

    fn invalidate_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App) {
        let entities: Vec<gpui::Entity<InfiniteQueryResource<T, E>>> = self
            .entries
            .iter()
            .filter_map(|(key, entry)| {
                if filter.matches(key) {
                    entry.entity.upgrade()
                } else {
                    None
                }
            })
            .collect();

        for entity in entities {
            entity.update(cx, |resource, _| {
                resource.invalidate();
            });
        }
    }

    fn reset_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App) {
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
    /// Only cancels entries that have an active request (status is loading).
    /// Uses the collect-then-update pattern to avoid mutating the `HashMap`
    /// during iteration.
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
                    let is_loading = entity.read_with(cx, |r, _| r.status().is_loading());
                    if is_loading {
                        entity.update(cx, |resource, _| {
                            // InfiniteQueryResource does not have a cancel method
                            // that transitions out of loading. The best we can do
                            // is reset, which clears pages. Alternatively, we could
                            // read the signal and cancel it so the in-flight fetch
                            // aborts, then let the completion handler see a stale
                            // request ID. For now, cancel the signal if present.
                            if let Some(signal) = resource.signal() {
                                signal.cancel();
                            }
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

    /// Collect per-resource diagnostic details for all live infinite query entries.
    fn collect_diagnostics(&self, now_ms: u128, cx: &App) -> Vec<QueryDiagnostic> {
        self.entries
            .iter()
            .filter_map(|(key, entry)| {
                let entity = entry.entity.upgrade()?;
                let resource = entity.read(cx);
                let age_ms = resource
                    .last_updated_at_ms()
                    .map(|updated| now_ms.saturating_sub(updated));
                Some(QueryDiagnostic {
                    key: key.to_path(),
                    status: resource.status(),
                    cache_policy: resource.cache_policy().label(),
                    cache_age_ms: age_ms,
                    cache_hits: resource.cache_hits(),
                    retry_count: resource.retry_count(),
                })
            })
            .collect()
    }
}
