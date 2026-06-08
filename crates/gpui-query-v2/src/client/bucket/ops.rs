//! Core operations for `QueryBucket`: construction, get-or-create, observers,
//! sequencer access, and status snapshot updates.

use ahash::AHashMap;
use gpui::{App, AppContext as _};

use crate::core::{
    CachePolicy, QueryKey, QueryResource, QueryStatus, RequestPolicy, RequestSequencer,
};

use super::types::{
    BucketEntry, DEFAULT_MAX_ENTRIES, StatusSnapshot,
};

/// Type-partitioned storage for query resources of a specific `(T, E)` type pair.
pub struct QueryBucket<T, E> {
    pub(crate) entries: AHashMap<QueryKey, BucketEntry<T, E>>,
    /// Maximum number of entries allowed in this bucket.
    /// When exceeded, the oldest entry (by `last_updated_ms`) is evicted.
    pub(crate) max_entries: usize,
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
    #[allow(dead_code)]
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
    pub(crate) fn evict_oldest(&mut self) {
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
