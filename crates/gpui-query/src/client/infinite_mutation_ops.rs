//! Infinite query, mutation, and bulk operations on `QueryClient`.
//!
//! This module contains `impl QueryClient` methods for:
//! - Infinite query resource management and lookups
//! - Mutation registration and lookups
//! - Bulk operations (invalidate/reset/remove/cancel) across all bucket types

use std::any::TypeId;

use gpui::{App, Entity};

use crate::core::{
    CachePolicy, InfiniteQueryResource, MutationResource, QueryKey, QueryKeyFilter,
    RequestPolicy,
};
use crate::client::infinite_bucket::InfiniteQueryBucket;
use crate::client::mutation_bucket::MutationBucket;

use super::QueryClient;

impl QueryClient {
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
}
