use std::any::TypeId;
use std::collections::HashMap;

use gpui::{App, AppContext, Entity};

use crate::core::{
    CachePolicy, QueryBeginResult, QueryFetchMode, QueryKey, QueryKeyFilter, QueryResource,
    QuerySignal, RequestId, RequestPolicy, RequestSequencer,
};
use crate::client::devtools::QueryDiagnostic;

/// Type-erased trait for bulk operations that don't need to know `T` or `E`.
///
/// Each [`QueryBucket`] implements this trait. The [`QueryClient`](super::QueryClient)
/// stores buckets as `Box<dyn QueryBucketTrait>` so it can iterate across all
/// type-partitioned buckets without knowing their concrete types.
pub trait QueryBucketTrait: Send + Sync {
    /// Invalidate (expire cache for) all resources matching the filter.
    fn invalidate_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App);

    /// Reset (clear all state for) all resources matching the filter.
    fn reset_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App);

    /// Remove resources matching the filter from the registry entirely.
    fn remove_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App);

    /// Remove resources that are idle and older than `gc_time_ms`.
    /// Resources with active requests are never collected.
    fn gc(&mut self, cx: &mut App, now_ms: u128, gc_time_ms: u64);

    /// Total number of resources in this bucket.
    fn count(&self) -> usize;

    /// Collect diagnostics for all resources in this bucket.
    fn collect_diagnostics(&self, cx: &App, out: &mut Vec<QueryDiagnostic>);
}

/// Default policies applied when creating new resources.
#[derive(Clone, Copy, Debug)]
pub struct BucketDefaults {
    pub cache_policy: CachePolicy,
    pub request_policy: RequestPolicy,
    pub gc_time_ms: u64,
}

/// A typed bucket storing [`QueryResource`] entities for one specific `(T, E)` type pair.
///
/// Each bucket also owns the [`RequestSequencer`]s for its resources, eliminating
/// the need for consumers to manage sequencers separately.
pub struct QueryBucket<T: 'static, E: 'static = crate::core::QueryError> {
    pub(crate) resources: HashMap<QueryKey, Entity<QueryResource<T, E>>>,
    sequencers: HashMap<QueryKey, RequestSequencer>,
    defaults: BucketDefaults,
}

impl<T: 'static, E: 'static> QueryBucket<T, E> {
    pub fn new(defaults: BucketDefaults) -> Self {
        Self {
            resources: HashMap::new(),
            sequencers: HashMap::new(),
            defaults,
        }
    }

    /// Get or create a [`QueryResource`] entity for the given key with default policies.
    pub fn resource(&mut self, key: QueryKey, cx: &mut App) -> Entity<QueryResource<T, E>> {
        self.resource_with_policies(
            key,
            self.defaults.cache_policy,
            self.defaults.request_policy,
            cx,
        )
    }

    /// Get or create a [`QueryResource`] entity with explicit policies.
    pub fn resource_with_policies(
        &mut self,
        key: QueryKey,
        cache_policy: CachePolicy,
        request_policy: RequestPolicy,
        cx: &mut App,
    ) -> Entity<QueryResource<T, E>> {
        if let Some(entity) = self.resources.get(&key) {
            return entity.clone();
        }
        let entity = cx.new(|_| QueryResource::new(key.clone(), cache_policy, request_policy));
        self.resources.insert(key.clone(), entity.clone());
        self.sequencers.insert(key, RequestSequencer::new());
        entity
    }

    /// Begin a request on the resource for `key`, using the bucket's co-located sequencer.
    ///
    /// Returns `None` if the key doesn't exist in this bucket.
    pub fn begin_request_for(
        &mut self,
        key: &QueryKey,
        now_ms: u128,
        fetch_mode: QueryFetchMode,
        cx: &mut App,
    ) -> Option<QueryBeginResult> {
        let entity = self.resources.get(key).cloned()?;
        let sequencer = self.sequencers.entry(key.clone()).or_default();
        Some(entity.update(cx, |resource, _| {
            resource.begin_request(sequencer, now_ms, fetch_mode)
        }))
    }

    /// Fetch (begin request) for a key, creating the resource if needed.
    ///
    /// This is a combined create-and-begin operation. If the key doesn't exist
    /// yet, it creates the resource with the given policies, then begins a request.
    /// Returns `Some((entity, request_id))` when a new request is started,
    /// or `None` if the request was short-circuited (CacheHit) or ignored
    /// (IgnoredWhileLoading).
    pub fn fetch(
        &mut self,
        key: &QueryKey,
        cache_policy: CachePolicy,
        request_policy: RequestPolicy,
        now_ms: u128,
        fetch_mode: QueryFetchMode,
        cx: &mut App,
    ) -> Option<(Entity<QueryResource<T, E>>, RequestId)> {
        let entity = self.resource_with_policies(key.clone(), cache_policy, request_policy, cx);
        let sequencer = self.sequencers.entry(key.clone()).or_default();
        let result = entity.update(cx, |resource, _| {
            resource.begin_request(sequencer, now_ms, fetch_mode)
        });
        match result {
            QueryBeginResult::Started { request_id, .. } => Some((entity, request_id)),
            QueryBeginResult::CacheHit => None,
            QueryBeginResult::IgnoredWhileLoading { .. } => None,
        }
    }

    /// Returns a clone of the cancellation signal for the resource at `key`,
    /// if the resource exists and has an active signal.
    ///
    /// This is useful for passing to an async fetcher so it can observe
    /// cooperative cancellation.
    pub fn signal_for(&self, key: &QueryKey, cx: &App) -> Option<QuerySignal> {
        let entity = self.resources.get(key)?;
        entity.read(cx).signal().cloned()
    }

    /// Optimistically set data on the resource at `key` without completing a request.
    ///
    /// The current data is stored in `previous_data` for potential rollback.
    /// Returns `true` if the resource was found and updated, `false` otherwise.
    pub fn set_data_for(&mut self, key: &QueryKey, data: T, cx: &mut App) -> bool {
        let Some(entity) = self.resources.get(key).cloned() else {
            return false;
        };
        entity.update(cx, |resource, _| {
            resource.set_data(data);
        });
        true
    }

    /// Remove resources matching the filter from this bucket entirely.
    ///
    /// Unlike [`reset_matching`](QueryBucket::reset_matching) which clears state
    /// but keeps the entities, this drops them from the registry along with
    /// their co-located sequencers.
    pub fn remove_matching(&mut self, filter: &QueryKeyFilter, _cx: &mut App) {
        let keys_to_remove: Vec<QueryKey> = self
            .resources
            .keys()
            .filter(|k| filter.matches(k))
            .cloned()
            .collect();
        for key in keys_to_remove {
            self.resources.remove(&key);
            self.sequencers.remove(&key);
        }
    }

    /// Roll back optimistically set data on the resource at `key`.
    ///
    /// Returns `true` if the resource was found and rollback succeeded
    /// (i.e., there was previous data to restore). Returns `false` if
    /// the resource wasn't found or had no previous data.
    pub fn rollback_data_for(&mut self, key: &QueryKey, cx: &mut App) -> bool {
        let Some(entity) = self.resources.get(key).cloned() else {
            return false;
        };
        entity.update(cx, |resource, _| resource.rollback_to_previous())
    }
}

impl<T: 'static, E: 'static> QueryBucketTrait for QueryBucket<T, E> {
    fn remove_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App) {
        QueryBucket::remove_matching(self, filter, cx);
    }

    fn invalidate_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App) {
        for (key, entity) in &self.resources {
            if filter.matches(key) {
                let entity = entity.clone();
                entity.update(cx, |resource, _| {
                    resource.invalidate();
                });
            }
        }
    }

    fn reset_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App) {
        for (key, entity) in &self.resources {
            if filter.matches(key) {
                let entity = entity.clone();
                entity.update(cx, |resource, _| {
                    resource.reset();
                });
            }
        }
    }

    fn gc(&mut self, cx: &mut App, now_ms: u128, gc_time_ms: u64) {
        self.resources.retain(|_key, entity| {
            let r = entity.read(cx);
            if r.active_request_id().is_some() {
                return true;
            }
            let Some(last) = r.last_updated_at_ms() else {
                return true;
            };
            let age = now_ms.saturating_sub(last);
            age <= gc_time_ms as u128
        });
        self.sequencers
            .retain(|key, _| self.resources.contains_key(key));
    }

    fn count(&self) -> usize {
        self.resources.len()
    }

    fn collect_diagnostics(&self, cx: &App, out: &mut Vec<QueryDiagnostic>) {
        for entity in self.resources.values() {
            let resource = entity.read(cx);
            out.push(QueryDiagnostic {
                key: resource.key().as_str().to_string(),
                status: resource.status().label().to_string(),
                has_data: resource.has_data(),
                has_error: resource.error().is_some(),
                cache_policy: resource.cache_policy().label(),
                request_policy: resource.request_policy().label().to_string(),
                cache_hits: resource.cache_hits(),
                cancelled_count: resource.cancelled_count(),
                ignored_results: resource.ignored_results(),
                last_updated_at_ms: resource.last_updated_at_ms(),
                started_at_ms: resource.started_at_ms(),
            });
        }
    }
}

// ── Type-erased storage helper ─────────────────────────────────────────

/// A type-erased bucket stored in [`QueryClient`](super::QueryClient).
/// Knows its `TypeId` for safe downcasting.
pub(crate) struct ErasedBucket {
    pub type_id: TypeId,
    pub bucket: Box<dyn QueryBucketTrait>,
}

impl ErasedBucket {
    pub fn new_typed<T: Clone + Send + Sync + 'static, E: Clone + Send + Sync + 'static>(
        bucket: QueryBucket<T, E>,
    ) -> Self {
        Self {
            type_id: TypeId::of::<(T, E)>(),
            bucket: Box::new(bucket),
        }
    }

    pub fn downcast_ref<T: 'static, E: 'static>(&self) -> Option<&QueryBucket<T, E>> {
        if self.type_id == TypeId::of::<(T, E)>() {
            // SAFETY: TypeId check guarantees the concrete type
            Some(unsafe {
                &*(self.bucket.as_ref() as *const dyn QueryBucketTrait as *const QueryBucket<T, E>)
            })
        } else {
            None
        }
    }

    pub fn downcast_mut<T: 'static, E: 'static>(&mut self) -> Option<&mut QueryBucket<T, E>> {
        if self.type_id == TypeId::of::<(T, E)>() {
            // SAFETY: TypeId check guarantees the concrete type
            Some(unsafe {
                &mut *(self.bucket.as_mut() as *mut dyn QueryBucketTrait as *mut QueryBucket<T, E>)
            })
        } else {
            None
        }
    }
}
