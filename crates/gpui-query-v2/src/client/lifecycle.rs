//! Lifecycle operations on `QueryClient`: GC, diagnostics, serialization,
//! persistence, and imperative fetch/prefetch.
//!
//! This module contains `impl QueryClient` methods for:
//! - Garbage collection (`gc`, `gc_with_time`)
//! - Test helpers for deterministic GC (snapshot updates, retain/release)
//! - Diagnostics
//! - Dehydration/hydration for state serialization
//! - Persistence via `QueryPersister`
//! - Imperative fetch and prefetch operations

use gpui::App;

use crate::core::{
    CachePolicy, QueryKey, QueryStatus, RequestPolicy,
};
use crate::client::bucket::QueryBucket;
use crate::client::devtools::{ClientDiagnostic, DehydratedEntry, DehydratedState};
use crate::client::erased::current_time_ms;
use crate::client::infinite_bucket::InfiniteQueryBucket;
use crate::client::prepared_fetch::PreparedFetch;

use super::QueryClient;
use crate::client::erased::QueryPersister;

impl QueryClient {
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
        let type_id = std::any::TypeId::of::<(T, E)>();
        if let Some(bucket) = self.buckets.get_mut(&type_id) {
            if let Some(typed) = bucket.as_any_mut().downcast_mut::<QueryBucket<T, E>>() {
                typed.update_status_snapshot(key, status, last_updated_ms, cache_policy);
            }
        }
    }

    /// Update the cached `StatusSnapshot` for an infinite query bucket entry.
    ///
    /// Test helper for deterministic GC tests on infinite queries. In production,
    /// the hook layer calls `bucket.update_status_snapshot()` after each request
    /// completion. In tests that bypass the hook layer, this method allows
    /// controlling the snapshot so GC behavior is deterministic.
    #[allow(dead_code)]
    pub(crate) fn update_infinite_snapshot<
        T: Clone + Send + Sync + 'static,
        E: Clone + Send + Sync + 'static,
    >(
        &mut self,
        key: &QueryKey,
        status: QueryStatus,
        last_updated_ms: Option<u128>,
        cache_policy: CachePolicy,
    ) {
        let type_id = std::any::TypeId::of::<(T, E)>();
        if let Some(bucket) = self.infinite_buckets.get_mut(&type_id) {
            if let Some(typed) = bucket.as_any_mut().downcast_mut::<InfiniteQueryBucket<T, E>>() {
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
        let type_id = std::any::TypeId::of::<(T, E)>();
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
        let type_id = std::any::TypeId::of::<(T, E)>();
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
        let type_id = std::any::TypeId::of::<(T, E)>();
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
        let type_id = std::any::TypeId::of::<(T, E)>();
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
