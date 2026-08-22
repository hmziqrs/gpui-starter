//! `ErasedBucket` trait implementation for `QueryBucket`.
//!
//! Contains garbage collection, bulk invalidation/reset/cancel, and
//! diagnostic collection — all methods dispatched through the type-erased
//! bucket trait.

use gpui::App;

use crate::client::devtools::QueryDiagnostic;
use crate::client::erased::ErasedBucket;
use crate::core::{QueryKey, QueryKeyFilter, QueryStatus};

use super::ops::QueryBucket;
use super::types::{MIN_GC_TIME_MS, SUCCESS_GC_MULTIPLIER};

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
