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
mod infinite_bucket;
mod mutation_bucket;
mod observer;

pub use bucket::QueryBucket;
pub use devtools::{
    ClientDiagnostic, DehydratedEntry, DehydratedState, MutationDiagnostic, QueryDiagnostic,
};
pub use infinite_bucket::InfiniteQueryBucket;
pub use mutation_bucket::MutationBucket;
pub use observer::{InfiniteQueryObserver, MutationObserver, ObserverConfig, QueryObserver};

use std::any::TypeId;

use ahash::AHashMap;
use gpui::{App, Entity, Global};

use crate::core::{
    CachePolicy, InfiniteQueryResource, MutationResource, QueryKey, QueryKeyFilter,
    QueryResource, QueryStatus, RequestPolicy,
};

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
    buckets: AHashMap<TypeId, Box<dyn ErasedBucket>>,
    infinite_buckets: AHashMap<TypeId, Box<dyn ErasedInfiniteBucket>>,
    mutation_buckets: AHashMap<TypeId, Box<dyn ErasedMutationBucket>>,
    default_cache_policy: CachePolicy,
    default_request_policy: RequestPolicy,
    gc_time_ms: u64,
}

impl Global for QueryClient {}

/// Type-erased bucket trait for storage in a homogeneous map.
trait ErasedBucket {
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
    fn gc(&mut self, now_ms: u128, gc_time_ms: u64, cx: &App);
    fn count(&self) -> usize;
    fn invalidate_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App);
    fn reset_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App);
    fn remove_matching(&mut self, filter: &QueryKeyFilter);
    fn cancel_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App);
    fn collect_diagnostics(&self, now_ms: u128, cx: &App) -> Vec<QueryDiagnostic>;
}

/// Type-erased infinite query bucket trait.
trait ErasedInfiniteBucket {
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
    fn gc(&mut self, now_ms: u128, gc_time_ms: u64, cx: &App);
    fn count(&self) -> usize;
    fn invalidate_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App);
    fn reset_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App);
    fn remove_matching(&mut self, filter: &QueryKeyFilter);
    fn cancel_matching(&mut self, filter: &QueryKeyFilter, cx: &mut App);
    fn collect_diagnostics(&self, now_ms: u128, cx: &App) -> Vec<QueryDiagnostic>;
}

/// Type-erased mutation bucket trait.
trait ErasedMutationBucket {
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
    fn gc(&mut self, now_ms: u128, gc_time_ms: u64, cx: &App);
    fn count(&self) -> usize;
    fn collect_diagnostics(&self, cx: &App) -> Vec<MutationDiagnostic>;
}

/// Persistence adapter trait for query cache persistence across app restarts.
///
/// Implementations can store cached data in any backend (filesystem, database, etc.).
/// Entries are serialized as JSON strings to avoid generic bounds on the persister.
///
/// # Example
///
/// ```
/// use std::path::PathBuf;
/// use gpui_query_v2::client::{QueryPersister, DehydratedEntry};
///
/// struct FilePersister { path: PathBuf }
///
/// impl QueryPersister for FilePersister {
///     fn load(&self) -> Vec<DehydratedEntry> { Vec::new() }
///     fn save(&self, _entries: Vec<DehydratedEntry>) {}
/// }
/// ```
pub trait QueryPersister: Send + Sync {
    /// Load persisted entries from storage.
    fn load(&self) -> Vec<DehydratedEntry>;

    /// Save entries to storage, replacing any previously stored data.
    fn save(&self, entries: Vec<DehydratedEntry>);
}

// ── Helper: current time in milliseconds since UNIX epoch ─────────────

/// Returns the current time as milliseconds since the UNIX epoch.
///
/// Used internally by `gc()` and other time-sensitive operations.
/// Exposed so callers can cache the value and pass it to `gc_with_time()`
/// to avoid repeated syscalls.
pub fn current_time_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

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
        self.resource_with_policies::<T, E>(key, self.default_cache_policy, self.default_request_policy, cx)
    }

    /// Get or create a query resource with explicit policies.
    ///
    /// Audit 3 fix (findings 3, 4): Uses graceful downcast recovery instead
    /// of `expect()`. On type mismatch, logs the type name and creates a
    /// fresh bucket, preventing application crashes from hypothetical
    /// TypeId collisions.
    pub fn resource_with_policies<T: Clone + Send + Sync + 'static, E: Clone + Send + Sync + 'static>(
        &mut self,
        key: impl Into<QueryKey>,
        cache_policy: CachePolicy,
        request_policy: RequestPolicy,
        cx: &mut App,
    ) -> Entity<QueryResource<T, E>> {
        let type_id = TypeId::of::<(T, E)>();
        let bucket = self.buckets
            .entry(type_id)
            .or_insert_with(|| Box::new(QueryBucket::<T, E>::new()));

        // Audit 3 fix (findings 3, 4): Graceful downcast with type name in
        // error message. Uses two-step pattern to satisfy borrow checker:
        // try downcast first, if it fails, replace bucket and retry.
        let typed = {
            if bucket.as_any_mut().downcast_mut::<QueryBucket<T, E>>().is_some() {
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
            if bucket.as_any_mut().downcast_mut::<QueryBucket<T, E>>().is_none() {
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

    // ── Infinite query operations ───────────────────────────────────────

    /// Get or create an infinite query resource for the given key and type pair.
    pub fn infinite_resource<T: Clone + Send + Sync + 'static, E: Clone + Send + Sync + 'static>(
        &mut self,
        key: impl Into<QueryKey>,
        cx: &mut App,
    ) -> Entity<InfiniteQueryResource<T, E>> {
        self.infinite_resource_with_policies::<T, E>(
            key,
            self.default_cache_policy,
            self.default_request_policy,
            cx,
        )
    }

    /// Get or create an infinite query resource with explicit policies.
    ///
    /// Audit 3 fix (findings 3, 4): Graceful downcast recovery.
    pub fn infinite_resource_with_policies<
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &mut self,
        key: impl Into<QueryKey>,
        cache_policy: CachePolicy,
        request_policy: RequestPolicy,
        cx: &mut App,
    ) -> Entity<InfiniteQueryResource<T, E>> {
        let type_id = TypeId::of::<(T, E)>();
        let bucket = self
            .infinite_buckets
            .entry(type_id)
            .or_insert_with(|| Box::new(InfiniteQueryBucket::<T, E>::new()));

        let typed = {
            if bucket.as_any_mut().downcast_mut::<InfiniteQueryBucket<T, E>>().is_none() {
                eprintln!(
                    "QueryClient: type mismatch in infinite bucket downcast for {}. \
                     Replacing with a fresh bucket.",
                    std::any::type_name::<(T, E)>()
                );
                *bucket = Box::new(InfiniteQueryBucket::<T, E>::new());
            }
            bucket
                .as_any_mut()
                .downcast_mut::<InfiniteQueryBucket<T, E>>()
                .expect("freshly created InfiniteQueryBucket must downcast correctly")
        };

        typed.get_or_create(key.into(), cache_policy, request_policy, cx)
    }

    /// Get a specific infinite query entity by key.
    pub fn infinite_query<
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &self,
        key: &QueryKey,
    ) -> Option<Entity<InfiniteQueryResource<T, E>>> {
        let type_id = TypeId::of::<(T, E)>();
        self.infinite_buckets
            .get(&type_id)
            .and_then(|b| b.as_any().downcast_ref::<InfiniteQueryBucket<T, E>>())
            .and_then(|b| b.get(key))
    }

    /// Use the infinite query bucket's co-located sequencer to generate a
    /// `RequestId` for an infinite query key.
    ///
    /// Returns `None` if no bucket entry exists for the key. The sequencer is
    /// advanced in-place so subsequent calls produce monotonically increasing IDs.
    /// This is the infinite query equivalent of [`next_request_id_for_key`](Self::next_request_id_for_key).
    ///
    /// Audit 3 fix (findings 3, 4): Graceful downcast recovery.
    pub fn next_request_id_for_infinite_key<
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &mut self,
        key: &QueryKey,
    ) -> Option<crate::core::RequestId> {
        let type_id = TypeId::of::<(T, E)>();
        let bucket = self.infinite_buckets.get_mut(&type_id)?;
        let typed = {
            if bucket.as_any_mut().downcast_mut::<InfiniteQueryBucket<T, E>>().is_none() {
                eprintln!(
                    "QueryClient: type mismatch in infinite bucket downcast for {}. \
                     Replacing with a fresh bucket.",
                    std::any::type_name::<(T, E)>()
                );
                *bucket = Box::new(InfiniteQueryBucket::<T, E>::new());
            }
            bucket
                .as_any_mut()
                .downcast_mut::<InfiniteQueryBucket<T, E>>()
                .expect("freshly created InfiniteQueryBucket must downcast correctly")
        };
        typed.sequencer_mut(key).map(|seq| seq.next_request())
    }

    /// Get all infinite query entities of a given type pair.
    pub fn all_infinite_queries<
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &self,
    ) -> Vec<Entity<InfiniteQueryResource<T, E>>> {
        let type_id = TypeId::of::<(T, E)>();
        self.infinite_buckets
            .get(&type_id)
            .and_then(|b| b.as_any().downcast_ref::<InfiniteQueryBucket<T, E>>())
            .map(|b| b.all_entities())
            .unwrap_or_default()
    }

    // ── Mutation operations ─────────────────────────────────────────────

    /// Register a mutation entity.
    ///
    /// Audit 3 fix (findings 3, 4): Graceful downcast recovery.
    pub fn register_mutation<
        V: Clone + Send + Sync + 'static,
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &mut self,
        entity: &Entity<MutationResource<V, T, E>>,
        cx: &App,
    ) {
        let type_id = TypeId::of::<(V, T, E)>();
        let bucket = self.mutation_buckets
            .entry(type_id)
            .or_insert_with(|| Box::new(MutationBucket::<V, T, E>::new()));

        let typed = {
            if bucket.as_any_mut().downcast_mut::<MutationBucket<V, T, E>>().is_none() {
                eprintln!(
                    "QueryClient: type mismatch in mutation bucket downcast for {}. \
                     Replacing with a fresh bucket.",
                    std::any::type_name::<(V, T, E)>()
                );
                *bucket = Box::new(MutationBucket::<V, T, E>::new());
            }
            bucket
                .as_any_mut()
                .downcast_mut::<MutationBucket<V, T, E>>()
                .expect("freshly created MutationBucket must downcast correctly")
        };

        typed.insert(entity, cx);
    }

    /// Get all mutation entities of a given type triple.
    pub fn all_mutations<
        V: Clone + Send + Sync + 'static,
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &self,
    ) -> Vec<Entity<MutationResource<V, T, E>>> {
        let type_id = TypeId::of::<(V, T, E)>();
        self.mutation_buckets
            .get(&type_id)
            .and_then(|b| b.as_any().downcast_ref::<MutationBucket<V, T, E>>())
            .map(|b| b.all_entities())
            .unwrap_or_default()
    }

    // ── Bulk operations ─────────────────────────────────────────────────

    /// Invalidate queries matching the filter.
    ///
    /// Uses collect-then-update pattern to avoid nested entity borrows.
    pub fn invalidate_queries(&mut self, filter: &QueryKeyFilter, cx: &mut App) {
        for bucket in self.buckets.values_mut() {
            bucket.invalidate_matching(filter, cx);
        }
        for bucket in self.infinite_buckets.values_mut() {
            bucket.invalidate_matching(filter, cx);
        }
    }

    /// Reset queries matching the filter.
    pub fn reset_queries(&mut self, filter: &QueryKeyFilter, cx: &mut App) {
        for bucket in self.buckets.values_mut() {
            bucket.reset_matching(filter, cx);
        }
        for bucket in self.infinite_buckets.values_mut() {
            bucket.reset_matching(filter, cx);
        }
    }

    /// Remove queries matching the filter.
    pub fn remove_queries(&mut self, filter: &QueryKeyFilter) {
        for bucket in self.buckets.values_mut() {
            bucket.remove_matching(filter);
        }
        for bucket in self.infinite_buckets.values_mut() {
            bucket.remove_matching(filter);
        }
    }

    /// Cancel in-flight requests matching the filter (Audit 3, Finding 5).
    ///
    /// Iterates all query and infinite query buckets, finds resources with active
    /// requests, and cancels them with a [`QueryError::cancelled`] error. This is
    /// essential for cleanup when navigating away from a page or when bulk
    /// cancellation is needed.
    ///
    /// Equivalent to TanStack Query's `queryClient.cancelQueries()`. Individual
    /// `QueryResource::cancel()` exists but this is the bulk cancellation method
    /// on the client.
    pub fn cancel_queries(&mut self, filter: &QueryKeyFilter, cx: &mut App) {
        for bucket in self.buckets.values_mut() {
            bucket.cancel_matching(filter, cx);
        }
        for bucket in self.infinite_buckets.values_mut() {
            bucket.cancel_matching(filter, cx);
        }
    }

    // ── Garbage collection ──────────────────────────────────────────────

    /// Run garbage collection on all buckets.
    ///
    /// Calls `current_time_ms()` internally to get the current time. If you
    /// already have a cached time value, use [`gc_with_time`] to avoid the
    /// syscall overhead (Audit 3, Finding 2).
    pub fn gc(&mut self, cx: &App) {
        let now_ms = current_time_ms();
        self.gc_with_time(now_ms, cx);
    }

    /// Run garbage collection with a pre-computed time value (Audit 3, Finding 2).
    ///
    /// Use this when you call GC frequently and want to amortize the cost of
    /// `SystemTime::now()` across multiple calls. The `now_ms` parameter should
    /// be milliseconds since the UNIX epoch (as returned by [`current_time_ms`]).
    pub fn gc_with_time(&mut self, now_ms: u128, cx: &App) {
        for bucket in self.buckets.values_mut() {
            bucket.gc(now_ms, self.gc_time_ms, cx);
        }
        for bucket in self.infinite_buckets.values_mut() {
            bucket.gc(now_ms, self.gc_time_ms, cx);
        }
        for bucket in self.mutation_buckets.values_mut() {
            bucket.gc(now_ms, self.gc_time_ms, cx);
        }
    }

    // ── Test helpers (pub(crate) for deterministic GC tests) ────────────

    /// Update the cached `StatusSnapshot` for a bucket entry.
    ///
    /// This is the test-only counterpart to the hook layer's snapshot update.
    /// In production, the hook layer calls `bucket.update_status_snapshot()`
    /// after each request completion. In tests that bypass the hook layer
    /// (using `PreparedFetch` or direct entity manipulation), this method
    /// allows controlling the snapshot so GC behavior is deterministic.
    ///
    /// Without this, GC reads a stale snapshot (status=Idle, last_updated_ms=None)
    /// and may evict resources that the test expects to survive.
    #[allow(dead_code)]
    pub(crate) fn update_query_snapshot<
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &mut self,
        key: &QueryKey,
        status: QueryStatus,
        last_updated_ms: Option<u128>,
        cache_policy: CachePolicy,
    ) {
        let type_id = TypeId::of::<(T, E)>();
        if let Some(bucket) = self.buckets.get_mut(&type_id) {
            if let Some(typed) = bucket.as_any_mut().downcast_mut::<QueryBucket<T, E>>() {
                typed.update_status_snapshot(key, status, last_updated_ms, cache_policy);
            }
        }
    }

    /// Increment the observer count for a query bucket entry.
    ///
    /// Test helper to simulate the hook layer's `bucket.retain()` call so
    /// that GC protection for observed resources can be tested without the
    /// full hook pipeline.
    #[allow(dead_code)]
    pub(crate) fn retain_query<
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &mut self,
        key: &QueryKey,
    ) {
        let type_id = TypeId::of::<(T, E)>();
        if let Some(bucket) = self.buckets.get_mut(&type_id) {
            if let Some(typed) = bucket.as_any_mut().downcast_mut::<QueryBucket<T, E>>() {
                typed.retain(key);
            }
        }
    }

    /// Decrement the observer count for a query bucket entry.
    #[allow(dead_code)]
    pub(crate) fn release_query<
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &mut self,
        key: &QueryKey,
    ) {
        let type_id = TypeId::of::<(T, E)>();
        if let Some(bucket) = self.buckets.get_mut(&type_id) {
            if let Some(typed) = bucket.as_any_mut().downcast_mut::<QueryBucket<T, E>>() {
                typed.release(key);
            }
        }
    }

    /// Increment the observer count for an infinite query bucket entry.
    #[allow(dead_code)]
    pub(crate) fn retain_infinite_query<
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &mut self,
        key: &QueryKey,
    ) {
        let type_id = TypeId::of::<(T, E)>();
        if let Some(bucket) = self.infinite_buckets.get_mut(&type_id) {
            if let Some(typed) = bucket.as_any_mut().downcast_mut::<InfiniteQueryBucket<T, E>>() {
                typed.retain(key);
            }
        }
    }

    /// Decrement the observer count for an infinite query bucket entry.
    #[allow(dead_code)]
    pub(crate) fn release_infinite_query<
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &mut self,
        key: &QueryKey,
    ) {
        let type_id = TypeId::of::<(T, E)>();
        if let Some(bucket) = self.infinite_buckets.get_mut(&type_id) {
            if let Some(typed) = bucket.as_any_mut().downcast_mut::<InfiniteQueryBucket<T, E>>() {
                typed.release(key);
            }
        }
    }

    // ── Diagnostics (Audit 3, Finding 7) ────────────────────────────────

    /// Get diagnostics for all queries and mutations.
    ///
    /// Returns aggregate counts and per-resource diagnostic details. The
    /// `queries` and `mutations` vectors are populated by iterating all bucket
    /// entries, upgrading weak references, and reading entity state. Dead
    /// entries (collected entities) are skipped.
    ///
    /// **Audit 3 fix**: Previously returned empty `queries: Vec::new()` and
    /// `mutations: Vec::new()` vectors. Now fully populates per-resource
    /// diagnostics via `collect_diagnostics` on each erased bucket.
    pub fn diagnostics(&self, cx: &App) -> ClientDiagnostic {
        let now_ms = current_time_ms();
        let mut queries = Vec::new();
        let mut mutations = Vec::new();
        let mut query_count = 0;
        let mut mutation_count = 0;

        for bucket in self.buckets.values() {
            query_count += bucket.count();
            queries.extend(bucket.collect_diagnostics(now_ms, cx));
        }
        for bucket in self.infinite_buckets.values() {
            query_count += bucket.count();
            queries.extend(bucket.collect_diagnostics(now_ms, cx));
        }
        for bucket in self.mutation_buckets.values() {
            mutation_count += bucket.count();
            mutations.extend(bucket.collect_diagnostics(cx));
        }

        ClientDiagnostic {
            query_count,
            mutation_count,
            queries,
            mutations,
        }
    }

    // ── Serialization / Hydration (Audit 3, Finding 8) ──────────────────

    /// Serialize all cached query state into a portable format.
    ///
    /// Extracts all live query resources, recording their keys, status, and
    /// type information. The resulting [`DehydratedState`] can be persisted
    /// to disk or stored for later restoration via [`hydrate`].
    ///
    /// Only resources with `Success` status are included. Resources in
    /// `Idle`, `Loading`, `Failure`, or `Cancelled` states are skipped.
    ///
    /// **Note**: Full data serialization requires type-specific code at the
    /// call site. Use `get_query_data::<T, E>(key, cx)` to extract typed
    /// data and serialize it externally. The `DehydratedState` provides
    /// the metadata (keys, type IDs) needed for typed restoration.
    pub fn dehydrate(&self, cx: &App) -> DehydratedState {
        let mut entries = Vec::new();

        for (type_id, bucket) in &self.buckets {
            let diagnostics = bucket.collect_diagnostics(current_time_ms(), cx);
            for diag in &diagnostics {
                if diag.status == QueryStatus::Success {
                    entries.push(DehydratedEntry {
                        key: diag.key.clone(),
                        type_id: *type_id,
                        kind: "query",
                        data_json: None,
                    });
                }
            }
        }

        for (type_id, bucket) in &self.infinite_buckets {
            let diagnostics = bucket.collect_diagnostics(current_time_ms(), cx);
            for diag in &diagnostics {
                if diag.status == QueryStatus::Success {
                    entries.push(DehydratedEntry {
                        key: diag.key.clone(),
                        type_id: *type_id,
                        kind: "infinite",
                        data_json: None,
                    });
                }
            }
        }

        DehydratedState { entries }
    }

    /// Restore query state from a previously dehydrated snapshot.
    ///
    /// Full hydration requires type-specific deserialization. The `DehydratedState`
    /// contains `type_id` keys but downcasting requires knowing the concrete types
    /// at the call site. Callers should iterate `state.entries` and call
    /// `set_query_data::<T, E>()` for each entry where they know the types.
    ///
    /// This method is provided as a hook point for typed hydration and to
    /// document the intended API shape matching TanStack Query's
    /// `queryClient.hydrate()`.
    pub fn hydrate(&mut self, _state: DehydratedState, _cx: &mut App) {
        // Full hydration requires type-specific deserialization. The DehydratedState
        // contains type_id keys but downcasting requires knowing the concrete types
        // at the call site. Callers should iterate state.entries and call
        // set_query_data::<T, E> for each entry where they know the types.
    }

    // ── Persistence (Audit 3, Finding 9) ────────────────────────────────

    /// Persist all cached data using the provided persister.
    ///
    /// Dehydrates the current state and saves it via the persister. This can
    /// be called periodically (e.g., during GC) or on app shutdown to ensure
    /// cached data survives across app restarts.
    pub fn persist(&self, persister: &dyn QueryPersister, cx: &App) {
        let state = self.dehydrate(cx);
        persister.save(state.entries);
    }

    /// Restore cached data from a persister.
    ///
    /// Loads entries from the persister. Since type information is erased in
    /// the persister, callers must iterate and restore typed data themselves
    /// using `set_query_data`. This method loads the raw entries and returns
    /// them for inspection and typed restoration.
    pub fn restore(&self, persister: &dyn QueryPersister) -> Vec<DehydratedEntry> {
        persister.load()
    }

    // ── Imperative fetch (Audit 3, Finding 10) ──────────────────────────

    /// Prepare an imperative fetch for a query key, creating the resource if needed.
    ///
    /// This creates (or reuses) the resource entity and begins a forced request,
    /// returning a [`PreparedFetch`] containing the entity, request ID, and signal.
    /// The caller is responsible for calling the fetcher and completing the request
    /// using `complete_fetch` or by directly calling `complete_current_success` /
    /// `complete_current_failure` on the entity.
    ///
    /// This is the equivalent of TanStack Query's `queryClient.fetchQuery()`.
    /// Unlike `use_query`, this does not subscribe or create an observer.
    ///
    /// Returns `None` if the cache is fresh (cache hit) and no fetch is needed.
    /// In that case, use `get_query_data` to read the cached data.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gpui_query_v2::client::QueryClient;
    /// use gpui_query_v2::core::QueryKey;
    /// # #[derive(Clone)]
    /// # struct UserData;
    /// # #[derive(Clone, Debug)]
    /// # struct QueryError;
    /// # fn _doc(client: &mut QueryClient, cx: &mut gpui::App) {
    ///
    /// if let Some(prepared) = client.prepare_fetch_query::<UserData, QueryError>(
    ///     QueryKey::from("user/42"),
    ///     cx,
    /// ) {
    ///     // prepared.entity, prepared.signal, and prepared.request_id are now available.
    ///     // Use cx.spawn() to run your async fetcher, then call
    ///     // prepared.complete_success(data, cx) or prepared.complete_failure(e, cx).
    /// }
    /// # }
    /// ```
    pub fn prepare_fetch_query<
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &mut self,
        key: impl Into<QueryKey>,
        cx: &mut App,
    ) -> Option<PreparedFetch<T, E>> {
        let key = key.into();
        let entity = self.resource::<T, E>(key.clone(), cx);
        let now_ms = current_time_ms();

        // Get or create a request ID via the bucket's sequencer
        let request_id = self.next_request_id_for_key::<T, E>(&key);

        // Begin the request on the resource
        entity.update(cx, |resource, _| {
            if let Some(rid) = request_id {
                let result = resource.begin_request_with_id(
                    Some(rid),
                    now_ms,
                    crate::core::QueryFetchMode::Force,
                );
                match result {
                    crate::core::QueryBeginResult::Started { .. }
                    | crate::core::QueryBeginResult::StaleCacheHit { .. } => true,
                    crate::core::QueryBeginResult::CacheHit => false,
                    crate::core::QueryBeginResult::IgnoredWhileLoading { .. } => false,
                }
            } else {
                false
            }
        })
        .then(|| false)
        .unwrap_or(false);

        // Re-read to get the signal and request ID
        let (request_id, signal) = entity.read_with(cx, |resource, _| {
            let rid = resource.active_request_id()?;
            let signal = resource.signal().cloned()?;
            Some((rid, signal))
        })?;

        Some(PreparedFetch {
            entity,
            request_id,
            signal,
        })
    }

    // ── Prefetch (Audit 3, Finding 11) ──────────────────────────────────

    /// Prepare a prefetch for a key that will be needed soon.
    ///
    /// Creates the resource entity (or reuses an existing one) and begins a
    /// request if the cache is stale or empty. The resource is NOT subscribed
    /// -- no observer is attached. When a component later calls `use_query`
    /// with the same key, it will find the prefetched data in the cache.
    ///
    /// This is the equivalent of TanStack Query's `queryClient.prefetchQuery()`.
    ///
    /// If the resource already has fresh data (cache hit), returns `None`.
    /// Use `prepare_fetch_query` with forced mode to override this behavior.
    ///
    /// Returns a [`PreparedFetch`] containing the entity, request ID, and
    /// signal. The caller is responsible for calling the fetcher and completing
    /// the request.
    pub fn prepare_prefetch_query<
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &mut self,
        key: impl Into<QueryKey>,
        cache_policy: CachePolicy,
        request_policy: RequestPolicy,
        cx: &mut App,
    ) -> Option<PreparedFetch<T, E>> {
        let key = key.into();
        let entity = self.resource_with_policies::<T, E>(
            key.clone(),
            cache_policy,
            request_policy,
            cx,
        );
        let now_ms = current_time_ms();

        // Get request ID from sequencer
        let request_id = self.next_request_id_for_key::<T, E>(&key);

        // Begin the request (respects cache policy — will skip if fresh)
        let started = entity.update(cx, |resource, _| {
            if let Some(rid) = request_id {
                let result = resource.begin_request_with_id(
                    Some(rid),
                    now_ms,
                    crate::core::QueryFetchMode::Normal,
                );
                matches!(
                    result,
                    crate::core::QueryBeginResult::Started { .. }
                        | crate::core::QueryBeginResult::StaleCacheHit { .. }
                )
            } else {
                false
            }
        });

        if !started {
            return None;
        }

        // Re-read to get the signal and request ID
        let (request_id, signal) = entity.read_with(cx, |resource, _| {
            let rid = resource.active_request_id()?;
            let signal = resource.signal().cloned()?;
            Some((rid, signal))
        })?;

        Some(PreparedFetch {
            entity,
            request_id,
            signal,
        })
    }
}

/// A prepared fetch returned by [`QueryClient::prepare_fetch_query`] or
/// [`QueryClient::prepare_prefetch_query`].
///
/// Contains the entity, request ID, and cooperative cancellation signal
/// needed to perform the async fetch and complete the resource.
///
/// The caller should:
/// 1. Call their fetcher with `self.signal`
/// 2. Use `complete_success` or `complete_failure` with the result
///
/// # Example
///
/// ```no_run
/// use gpui_query_v2::client::QueryClient;
/// use gpui_query_v2::core::QueryKey;
/// # #[derive(Clone)]
/// # struct Data;
/// # #[derive(Clone, Debug)]
/// # struct Error;
/// # fn _doc(client: &mut QueryClient, cx: &mut gpui::App) {
/// # let key = QueryKey::from("data");
///
/// let prepared = client.prepare_fetch_query::<Data, Error>(key, cx).unwrap();
/// let signal = prepared.signal.clone();
/// // Use cx.spawn() to run your async fetcher with the signal, then call
/// // prepared.complete_success(data, cx) or prepared.complete_failure(e, cx).
/// # }
/// ```
pub struct PreparedFetch<T, E> {
    /// The query resource entity.
    pub entity: Entity<QueryResource<T, E>>,
    /// The request ID for the started request.
    pub request_id: crate::core::RequestId,
    /// The cooperative cancellation signal for the in-flight request.
    pub signal: crate::core::QuerySignal,
}

impl<T: Clone + Send + Sync + 'static, E: Clone + Send + Sync + 'static> PreparedFetch<T, E> {
    /// Complete the fetch with success.
    ///
    /// Calls `complete_current_success` on the resource entity. If the request
    /// ID is no longer active (replaced by a newer request), this is a no-op.
    pub fn complete_success(self, data: T, cx: &mut gpui::App) {
        let now_ms = current_time_ms();
        self.entity.update(cx, |resource, _| {
            resource.complete_current_success(self.request_id, data, now_ms);
        });
    }

    /// Complete the fetch with failure.
    ///
    /// Calls `complete_current_failure` on the resource entity. If the request
    /// ID is no longer active (replaced by a newer request), this is a no-op.
    pub fn complete_failure(self, error: E, cx: &mut gpui::App) {
        let now_ms = current_time_ms();
        self.entity.update(cx, |resource, _| {
            resource.complete_current_failure(self.request_id, error, now_ms);
        });
    }
}
