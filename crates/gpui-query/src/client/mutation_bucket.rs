//! Type-partitioned mutation bucket storage for [`MutationResource`].
//!
//! Mirrors the bucket pattern from [`crate::client::bucket`] but for mutation
//! resources. Each `(V, T, E)` type triple gets its own [`MutationBucket`],
//! stored in the [`QueryClient`](super::QueryClient) via type-erased
//! [`ErasedMutationBucket`].

use std::any::TypeId;

use gpui::{App, AppContext, Entity};

use crate::core::{MutationResource, QueryError, QueryKey, RetryPolicy};

/// Type-erased trait for mutation bucket bulk operations.
pub trait MutationBucketTrait: Send + Sync {
    /// Total number of mutation resources in this bucket.
    fn count(&self) -> usize;

    /// Remove mutation resources that are idle (not loading) and older than
    /// `gc_time_ms`. Resources with active signals are never collected.
    fn gc(&mut self, cx: &mut App, now_ms: u128, gc_time_ms: u64);
}

/// Default policies applied when creating new mutation resources.
#[derive(Clone, Debug)]
pub struct MutationDefaults {
    pub retry_policy: RetryPolicy,
    pub gc_time_ms: u64,
}

/// A typed bucket storing [`MutationResource`] entities for one specific
/// `(V, T, E)` type triple.
pub struct MutationBucket<V, T, E = QueryError> {
    resources: HashMap<QueryKey, Entity<MutationResource<V, T, E>>>,
    defaults: MutationDefaults,
}

use std::collections::HashMap;

impl<V: 'static, T: 'static, E: 'static> MutationBucket<V, T, E> {
    /// Create a new mutation bucket with the given defaults.
    pub fn new(defaults: MutationDefaults) -> Self {
        Self {
            resources: HashMap::new(),
            defaults,
        }
    }

    /// Get or create a [`MutationResource`] entity for the given key with
    /// default retry policy.
    pub fn resource(&mut self, key: &QueryKey, cx: &mut App) -> Entity<MutationResource<V, T, E>> {
        self.resource_with_policy(key, self.defaults.retry_policy.clone(), cx)
    }

    /// Get or create a [`MutationResource`] entity with an explicit retry policy.
    pub fn resource_with_policy(
        &mut self,
        key: &QueryKey,
        retry_policy: RetryPolicy,
        cx: &mut App,
    ) -> Entity<MutationResource<V, T, E>> {
        if let Some(entity) = self.resources.get(key) {
            return entity.clone();
        }
        let entity = cx.new(|_| MutationResource::new(retry_policy));
        self.resources.insert(key.clone(), entity.clone());
        entity
    }

    /// Check whether a mutation resource exists for the given key.
    pub fn contains(&self, key: &QueryKey) -> bool {
        self.resources.contains_key(key)
    }

    /// Total number of mutation resources in this bucket.
    pub fn count(&self) -> usize {
        self.resources.len()
    }

    /// Get all mutation resource entities in this bucket.
    pub fn all_entities(&self) -> Vec<Entity<MutationResource<V, T, E>>> {
        self.resources.values().cloned().collect()
    }

    /// Garbage-collect idle mutation resources.
    ///
    /// A resource is **retained** when any of the following is true:
    ///
    /// - It is currently in the `Loading` state (in-flight work).
    /// - It was created within the last `gc_time_ms` milliseconds.
    ///
    /// All other resources are removed. This mirrors the GC policy used by
    /// [`QueryBucket::gc`](crate::client::bucket::QueryBucket::gc) but uses
    /// the mutation's `created_at` timestamp instead of `last_updated_at`,
    /// since mutations track their creation time rather than a generic update
    /// timestamp.
    pub fn gc(&mut self, cx: &mut App, now_ms: u128, gc_time_ms: u64) {
        self.resources.retain(|_key, entity| {
            let r = entity.read(cx);
            // Keep if currently loading — in-flight work must not be collected.
            if r.is_loading() {
                return true;
            }
            // Keep if the resource was created within the GC window.
            let created_at = r.created_at() as u128;
            let age = now_ms.saturating_sub(created_at);
            age <= gc_time_ms as u128
        });
    }
}

impl<V: 'static, T: 'static, E: 'static> MutationBucketTrait for MutationBucket<V, T, E> {
    fn count(&self) -> usize {
        self.resources.len()
    }

    fn gc(&mut self, cx: &mut App, now_ms: u128, gc_time_ms: u64) {
        MutationBucket::gc(self, cx, now_ms, gc_time_ms)
    }
}

// ── Type-erased storage helper ─────────────────────────────────────────

/// A type-erased mutation bucket stored in [`QueryClient`](super::QueryClient).
/// Knows its `TypeId` for safe downcasting.
pub(crate) struct ErasedMutationBucket {
    pub type_id: TypeId,
    pub bucket: Box<dyn MutationBucketTrait>,
}

impl ErasedMutationBucket {
    pub fn new_typed<
        V: Clone + Send + Sync + 'static,
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        bucket: MutationBucket<V, T, E>,
    ) -> Self {
        Self {
            type_id: TypeId::of::<(V, T, E)>(),
            bucket: Box::new(bucket),
        }
    }

    pub fn downcast_ref<V: 'static, T: 'static, E: 'static>(
        &self,
    ) -> Option<&MutationBucket<V, T, E>> {
        if self.type_id == TypeId::of::<(V, T, E)>() {
            // SAFETY: TypeId check guarantees the concrete type
            Some(unsafe {
                &*(self.bucket.as_ref() as *const dyn MutationBucketTrait
                    as *const MutationBucket<V, T, E>)
            })
        } else {
            None
        }
    }

    pub fn downcast_mut<V: 'static, T: 'static, E: 'static>(
        &mut self,
    ) -> Option<&mut MutationBucket<V, T, E>> {
        if self.type_id == TypeId::of::<(V, T, E)>() {
            // SAFETY: TypeId check guarantees the concrete type
            Some(unsafe {
                &mut *(self.bucket.as_mut() as *mut dyn MutationBucketTrait
                    as *mut MutationBucket<V, T, E>)
            })
        } else {
            None
        }
    }
}
