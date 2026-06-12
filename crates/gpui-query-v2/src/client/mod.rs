//! Layer 1: GPUI `QueryClient` — global registry for query resources.
//!
//! `QueryClient` is a GPUI [`Global`] that manages type-partitioned buckets
//! for queries, mutations, and observers. It provides bulk operations like
//! `invalidate_queries`, `cancel_queries`, and garbage collection.
//!
//! # Audit 3 fixes
//!
//! - `gc()` accepts optional `now_ms` parameter via `gc_with_time()` to avoid
//!   redundant syscalls (finding 2)
//! - `expect()` on TypeId downcast replaced with graceful recovery + type name
//!   in error message (findings 3, 4)
//! - `cancel_queries()` added for bulk in-flight request cancellation (finding 5)
//! - `get_query_data()` / `set_query_data()` for ergonomic cache access (finding 6)
//! - `diagnostics()` now populates per-resource diagnostic details (finding 7)
//! - `dehydrate()` / `hydrate()` for state serialization across restarts (finding 8)
//! - `QueryPersister` trait and `persist()` / `restore()` for pluggable persistence (finding 9)
//! - `fetch_query()` for imperative one-shot fetches (finding 10)
//! - `prefetch_query()` for background cache warming (finding 11)

mod bucket;
mod devtools;
mod erased;
mod infinite_bucket;
mod infinite_mutation_ops;
mod lifecycle;
mod mutation_bucket;
mod observer;
mod prepared_fetch;

pub use bucket::QueryBucket;
pub use devtools::{
    ClientDiagnostic, DehydratedEntry, DehydratedState, MutationDiagnostic, QueryDiagnostic,
};
pub use erased::{QueryPersister, current_time_ms};
pub use infinite_bucket::InfiniteQueryBucket;
pub use mutation_bucket::MutationBucket;
pub use observer::{InfiniteQueryObserver, MutationObserver, ObserverConfig, QueryObserver};
pub use prepared_fetch::PreparedFetch;

use std::any::TypeId;

use ahash::AHashMap;
use gpui::{App, Entity, Global};

use crate::client::erased::{ErasedBucket, ErasedInfiniteBucket, ErasedMutationBucket};
use crate::core::{CachePolicy, QueryKey, QueryResource, RequestPolicy};

/// Global registry for query and mutation resources.
///
/// Implements [`Global`] so it can be set once with `cx.set_global(QueryClient::default())`
/// and accessed from any component via `cx.global::<QueryClient>()`.
///
/// # v2 Improvements
///
/// - `Default` impl (no required params)
/// - `AHashMap` for ~2x faster lookups on trusted keys
/// - Actual mutation GC (not a no-op)
/// - Collect-then-update pattern to avoid nested entity borrows
#[derive(Default)]
pub struct QueryClient {
    pub(crate) buckets: AHashMap<TypeId, Box<dyn ErasedBucket>>,
    pub(crate) infinite_buckets: AHashMap<TypeId, Box<dyn ErasedInfiniteBucket>>,
    pub(crate) mutation_buckets: AHashMap<TypeId, Box<dyn ErasedMutationBucket>>,
    pub(crate) default_cache_policy: CachePolicy,
    pub(crate) default_request_policy: RequestPolicy,
    pub(crate) gc_time_ms: u64,
}

impl Global for QueryClient {}

impl QueryClient {
    /// Create a new client with default policies.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with custom default policies.
    pub fn with_policies(
        default_cache_policy: CachePolicy,
        default_request_policy: RequestPolicy,
    ) -> Self {
        Self {
            default_cache_policy,
            default_request_policy,
            gc_time_ms: 300_000, // 5 minutes
            ..Default::default()
        }
    }

    /// Set the garbage collection time (in milliseconds).
    ///
    /// Values below 1000ms are clamped to 1000ms during GC to prevent
    /// aggressive eviction of all Idle/Failure resources on every GC pass.
    pub fn with_gc_time(mut self, gc_time_ms: u64) -> Self {
        self.gc_time_ms = gc_time_ms;
        self
    }

    // ── Query operations ────────────────────────────────────────────────

    /// Get or create a query resource for the given key and type pair.
    pub fn resource<T: Clone + Send + Sync + 'static, E: Clone + Send + Sync + 'static>(
        &mut self,
        key: impl Into<QueryKey>,
        cx: &mut App,
    ) -> Entity<QueryResource<T, E>> {
        self.resource_with_policies::<T, E>(
            key,
            self.default_cache_policy,
            self.default_request_policy,
            cx,
        )
    }

    /// Get or create a query resource with explicit policies.
    ///
    /// Audit 3 fix (findings 3, 4): Uses graceful downcast recovery instead
    /// of `expect()`. On type mismatch, logs the type name and creates a
    /// fresh bucket, preventing application crashes from hypothetical
    /// TypeId collisions.
    pub fn resource_with_policies<
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &mut self,
        key: impl Into<QueryKey>,
        cache_policy: CachePolicy,
        request_policy: RequestPolicy,
        cx: &mut App,
    ) -> Entity<QueryResource<T, E>> {
        let type_id = TypeId::of::<(T, E)>();
        let bucket = self
            .buckets
            .entry(type_id)
            .or_insert_with(|| Box::new(QueryBucket::<T, E>::new()));

        // Audit 3 fix (findings 3, 4): Graceful downcast with type name in
        // error message. Uses two-step pattern to satisfy borrow checker:
        // try downcast first, if it fails, replace bucket and retry.
        let typed = {
            if bucket
                .as_any_mut()
                .downcast_mut::<QueryBucket<T, E>>()
                .is_some()
            {
                // Downcast succeeded — borrow released by this point.
            } else {
                eprintln!(
                    "QueryClient: type mismatch in bucket downcast for {}. \
                     Replacing with a fresh bucket.",
                    std::any::type_name::<(T, E)>()
                );
                // Replace the mismatched bucket with a fresh one.
                *bucket = Box::new(QueryBucket::<T, E>::new());
            }
            // Now borrow again for the actual downcast (will always succeed).
            bucket
                .as_any_mut()
                .downcast_mut::<QueryBucket<T, E>>()
                .expect("freshly created QueryBucket must downcast correctly")
        };

        typed.get_or_create(key.into(), cache_policy, request_policy, cx)
    }

    /// Get all query entities of a given type pair.
    pub fn all_queries<T: Clone + Send + Sync + 'static, E: Clone + Send + Sync + 'static>(
        &self,
    ) -> Vec<Entity<QueryResource<T, E>>> {
        let type_id = TypeId::of::<(T, E)>();
        self.buckets
            .get(&type_id)
            .and_then(|b| b.as_any().downcast_ref::<QueryBucket<T, E>>())
            .map(|b| b.all_entities())
            .unwrap_or_default()
    }

    /// Get a specific query entity by key.
    pub fn query<T: Clone + Send + Sync + 'static, E: Clone + Send + Sync + 'static>(
        &self,
        key: &QueryKey,
    ) -> Option<Entity<QueryResource<T, E>>> {
        let type_id = TypeId::of::<(T, E)>();
        self.buckets
            .get(&type_id)
            .and_then(|b| b.as_any().downcast_ref::<QueryBucket<T, E>>())
            .and_then(|b| b.get(key))
    }

    /// Use the bucket's co-located sequencer to generate a `RequestId` for a key.
    ///
    /// Returns `None` if no bucket entry exists for the key. The sequencer is
    /// advanced in-place (mutated) so subsequent calls produce monotonically
    /// increasing IDs. This is the fix for audit findings #1/#5/#15/#18:
    /// using the bucket's persistent sequencer instead of a transient one
    /// prevents every request from getting the same `RequestId(1, 1)`.
    ///
    /// Audit 3 fix (findings 3, 4): Graceful downcast recovery.
    pub fn next_request_id_for_key<
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &mut self,
        key: &QueryKey,
    ) -> Option<crate::core::RequestId> {
        let type_id = TypeId::of::<(T, E)>();
        let bucket = self.buckets.get_mut(&type_id)?;
        // Audit 3 fix (findings 3, 4): Two-step downcast with graceful recovery.
        let typed = {
            if bucket
                .as_any_mut()
                .downcast_mut::<QueryBucket<T, E>>()
                .is_none()
            {
                eprintln!(
                    "QueryClient: type mismatch in bucket downcast for {}. \
                     Replacing with a fresh bucket.",
                    std::any::type_name::<(T, E)>()
                );
                *bucket = Box::new(QueryBucket::<T, E>::new());
            }
            bucket
                .as_any_mut()
                .downcast_mut::<QueryBucket<T, E>>()
                .expect("freshly created QueryBucket must downcast correctly")
        };
        typed.sequencer_mut(key).map(|seq| seq.next_request())
    }

    // ── Data accessors (Audit 3, Finding 6) ─────────────────────────────

    /// Read the cached data for a query key directly, without going through a hook.
    ///
    /// Returns `None` if no resource exists for the key, the entity was collected,
    /// or the resource has no data (has not completed a fetch).
    ///
    /// This is the ergonomic equivalent of TanStack Query's `queryClient.getQueryData(key)`.
    pub fn get_query_data<T: Clone + Send + Sync + 'static, E: Clone + Send + Sync + 'static>(
        &self,
        key: &QueryKey,
        cx: &App,
    ) -> Option<T> {
        let entity = self.query::<T, E>(key)?;
        entity.read_with(cx, |resource, _| resource.data().cloned())
    }

    /// Write data directly into the cache for a query key, creating the resource
    /// if it does not already exist.
    ///
    /// This is the ergonomic equivalent of TanStack Query's `queryClient.setQueryData(key, data)`.
    /// The resource's previous data is saved for rollback via `rollback_to_previous()`.
    /// The data is set via `set_data()` which saves previous data but does not
    /// change the resource's status or timestamp. Use this for optimistic updates
    /// and manual cache manipulation where you control the lifecycle.
    pub fn set_query_data<T: Clone + Send + Sync + 'static, E: Clone + Send + Sync + 'static>(
        &mut self,
        key: impl Into<QueryKey>,
        data: T,
        cx: &mut App,
    ) {
        let key = key.into();
        let entity = self.resource::<T, E>(key, cx);
        entity.update(cx, |resource, _| {
            resource.set_data(data);
        });
    }
}
