//! Integration tests for the QueryClient layer (v2).
//!
//! Tests use `#[gpui::test]` with `TestAppContext` and the `test_support` helpers.
//! They exercise the full client API: resource creation, type partitioning,
//! invalidation, reset, GC, mutations, diagnostics, signals, data access,
//! and observers.
//!
//! # Context pattern
//!
//! All tests use `cx.update_global::<QueryClient, _>(|client, cx| ...)` to
//! get `(&mut QueryClient, &mut App)`. Methods like `resource()` require
//! `&mut self` and `&mut App`, so `cx.global()` (immutable) cannot be used.
//!
//! # GC test design
//!
//! The bucket's GC reads a cached `StatusSnapshot` (not the live entity).
//! Direct entity manipulation (`apply_success`, etc.) and `PreparedFetch`
//! completions update the entity but do NOT update the bucket snapshot.
//! The snapshot is only updated by the hook layer in production.
//!
//! For deterministic GC tests, we use `client.update_query_snapshot()` to
//! simulate what the hook layer would do: set the snapshot status and
//! `last_updated_ms` to known values. This lets us assert exact eviction
//! and preservation behavior without the hook layer.

use std::sync::Mutex;

use gpui::{AppContext as _, BorrowAppContext as _, TestAppContext};

use crate::client::{
    MutationObserver, ObserverConfig, QueryClient, QueryObserver,
};
use crate::core::*;
use crate::tests::test_support::*;

// ── 1. QueryClient creation and Global registration ────────────────────

#[gpui::test]
fn test_client_creation_and_global_registration(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        let diag = cx.update_global::<QueryClient, _>(|client, cx| client.diagnostics(cx));
        assert_eq!(diag.query_count, 0, "new client should have zero queries");
        assert_eq!(
            diag.mutation_count, 0,
            "new client should have zero mutations"
        );
    });
}

#[gpui::test]
fn test_client_with_custom_policies(cx: &mut TestAppContext) {
    setup_query_client_with_policies(
        cx,
        CachePolicy::Ttl { ttl_ms: 5_000 },
        RequestPolicy::LatestWins,
    );
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = client.resource::<String, QueryError>("test_key", cx);
            entity.read_with(cx, |r, _| {
                assert_eq!(r.cache_policy(), CachePolicy::Ttl { ttl_ms: 5_000 });
            });
        });
    });
}

#[gpui::test]
fn test_client_with_gc_time(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = client.resource::<String, QueryError>("key", cx);
            entity.update(cx, |r, _| {
                r.apply_success("hello".to_string(), 100);
            });
            // GC at t=3000: success, age=2900 > success_threshold(2*1000=2000) -> evicted
            client.gc_with_time(3_000, cx);
            let remaining = client.all_queries::<String, QueryError>();
            assert!(
                remaining.is_empty(),
                "resource should be evicted by GC (age 2900 > success_threshold 2000)"
            );
        });
    });
}

// ── 2. resource() creates and retrieves typed entities ──────────────────

#[gpui::test]
fn test_resource_creates_and_deduplicates(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("user:1");

            let e1 = client.resource::<String, QueryError>(key.clone(), cx);
            assert_eq!(e1.read(cx).status(), QueryStatus::Idle);

            // Same key returns same entity (deduplication)
            let e2 = client.resource::<String, QueryError>(key.clone(), cx);
            assert_eq!(
                e1.entity_id(),
                e2.entity_id(),
                "same key should return same entity"
            );

            // Different key creates new entity
            let e3 = client.resource::<String, QueryError>("user:2", cx);
            assert_ne!(
                e1.entity_id(),
                e3.entity_id(),
                "different key should return different entity"
            );
        });
    });
}

#[gpui::test]
fn test_resource_with_explicit_policies(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = client.resource_with_policies::<String, QueryError>(
                "post:1",
                CachePolicy::StaleWhileRevalidate {
                    ttl_ms: 1_000,
                    stale_ms: 2_000,
                },
                RequestPolicy::IgnoreWhileLoading,
                cx,
            );
            entity.read_with(cx, |r, _| {
                assert_eq!(
                    r.cache_policy(),
                    CachePolicy::StaleWhileRevalidate {
                        ttl_ms: 1_000,
                        stale_ms: 2_000,
                    }
                );
                assert_eq!(r.request_policy(), RequestPolicy::IgnoreWhileLoading);
            });
        });
    });
}

#[gpui::test]
fn test_query_retrieves_existing_entity(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("item:1");

            let created = client.resource::<String, QueryError>(key.clone(), cx);

            // query() returns Some for existing key
            let retrieved = client.query::<String, QueryError>(&key);
            assert!(retrieved.is_some(), "should find existing key");
            assert_eq!(created.entity_id(), retrieved.unwrap().entity_id());

            // query() returns None for missing key
            let missing = client.query::<String, QueryError>(&QueryKey::from("nope"));
            assert!(missing.is_none(), "should not find nonexistent key");
        });
    });
}

// ── 3. Type-partitioned buckets: different (T,E) types don't conflict ──

#[gpui::test]
fn test_type_partitioned_buckets_no_conflict(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Same key "data" but different types — must not conflict
            let string_entity = client.resource::<String, QueryError>("data", cx);
            let u32_entity = client.resource::<u32, QueryError>("data", cx);
            let user_entity = client.resource::<User, QueryError>("data", cx);

            assert_ne!(
                string_entity.entity_id(),
                u32_entity.entity_id(),
                "different T types must produce different entities"
            );
            assert_ne!(
                string_entity.entity_id(),
                user_entity.entity_id(),
                "String and User must be separate"
            );
            assert_ne!(
                u32_entity.entity_id(),
                user_entity.entity_id(),
                "u32 and User must be separate"
            );

            // all_queries::<String, _> returns only String entities
            let strings = client.all_queries::<String, QueryError>();
            assert_eq!(strings.len(), 1);
            assert_eq!(strings[0].entity_id(), string_entity.entity_id());

            // all_queries::<User, _> returns only User entities
            let users = client.all_queries::<User, QueryError>();
            assert_eq!(users.len(), 1);
            assert_eq!(users[0].entity_id(), user_entity.entity_id());
        });
    });
}

#[gpui::test]
fn test_same_type_different_error_types_no_conflict(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Same T but different E — these should be in separate buckets
            let e1 = client.resource::<String, QueryError>("key", cx);
            let e2 = client.resource::<String, String>("key", cx);

            assert_ne!(
                e1.entity_id(),
                e2.entity_id(),
                "different E types must produce different entities"
            );
        });
    });
}

// ── 4. invalidate_queries with Exact/Prefix/All filters ───────────────

#[gpui::test]
fn test_invalidate_queries_exact_filter(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key1 = QueryKey::from(["users", "1"]);
            let key2 = QueryKey::from(["users", "2"]);

            let e1 = client.resource::<String, QueryError>(key1.clone(), cx);
            let e2 = client.resource::<String, QueryError>(key2.clone(), cx);

            e1.update(cx, |r, _| r.apply_success("Alice".to_string(), 1_000));
            e2.update(cx, |r, _| r.apply_success("Bob".to_string(), 1_000));

            assert!(
                e1.read(cx).is_cache_fresh(1_500),
                "should be fresh before invalidate"
            );
            assert!(e2.read(cx).is_cache_fresh(1_500));

            // Invalidate only users/1
            client.invalidate_queries(&QueryKeyFilter::Exact(&key1), cx);

            assert!(
                !e1.read(cx).is_cache_fresh(1_500),
                "e1 should be stale after exact invalidate"
            );
            assert!(
                e1.read(cx).data().is_some(),
                "data should survive invalidation"
            );
            assert!(
                e2.read(cx).is_cache_fresh(1_500),
                "e2 should remain fresh"
            );
        });
    });
}

#[gpui::test]
fn test_invalidate_queries_prefix_filter_across_types(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let u1 = client
                .resource::<String, QueryError>(QueryKey::from(["users", "1"]), cx);
            let u2 = client
                .resource::<String, QueryError>(QueryKey::from(["users", "2"]), cx);
            let p1 = client
                .resource::<String, QueryError>(QueryKey::from(["posts", "1"]), cx);

            u1.update(cx, |r, _| r.apply_success("user1".to_string(), 1_000));
            u2.update(cx, |r, _| r.apply_success("user2".to_string(), 1_000));
            p1.update(cx, |r, _| r.apply_success("post1".to_string(), 1_000));

            // Invalidate all "users" — posts unaffected
            let prefix = QueryKey::from(["users"]);
            client.invalidate_queries(&QueryKeyFilter::Prefix(&prefix), cx);

            assert!(!u1.read(cx).is_cache_fresh(1_500), "users/1 should be stale");
            assert!(!u2.read(cx).is_cache_fresh(1_500), "users/2 should be stale");
            assert!(
                p1.read(cx).is_cache_fresh(1_500),
                "posts/1 should still be fresh"
            );
        });
    });
}

#[gpui::test]
fn test_invalidate_queries_all_filter(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let u = client.resource::<String, QueryError>("user", cx);
            let p = client.resource::<String, QueryError>("post", cx);

            u.update(cx, |r, _| r.apply_success("u".to_string(), 1_000));
            p.update(cx, |r, _| r.apply_success("p".to_string(), 1_000));

            client.invalidate_queries(&QueryKeyFilter::All, cx);

            assert!(!u.read(cx).is_cache_fresh(1_500));
            assert!(!p.read(cx).is_cache_fresh(1_500));
        });
    });
}

// ── 5. reset_queries clears data across matching resources ──────────────

#[gpui::test]
fn test_reset_queries_clears_data_and_status(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let e1 = client.resource::<String, QueryError>("key1", cx);
            let e2 = client.resource::<String, QueryError>("key2", cx);

            e1.update(cx, |r, _| r.apply_success("data1".to_string(), 1_000));
            e2.update(cx, |r, _| r.apply_success("data2".to_string(), 1_000));

            assert!(e1.read(cx).data().is_some());
            assert!(e2.read(cx).data().is_some());

            client.reset_queries(&QueryKeyFilter::All, cx);

            assert!(
                e1.read(cx).data().is_none(),
                "data should be cleared after reset"
            );
            assert_eq!(e1.read(cx).status(), QueryStatus::Idle);
            assert!(e2.read(cx).data().is_none());
            assert_eq!(e2.read(cx).status(), QueryStatus::Idle);
        });
    });
}

#[gpui::test]
fn test_reset_queries_prefix_preserves_non_matching(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let u1 = client.resource::<String, QueryError>(
                QueryKey::from(["users", "1"]),
                cx,
            );
            let p1 = client.resource::<String, QueryError>(
                QueryKey::from(["posts", "1"]),
                cx,
            );

            u1.update(cx, |r, _| r.apply_success("user".to_string(), 1_000));
            p1.update(cx, |r, _| r.apply_success("post".to_string(), 1_000));

            let prefix = QueryKey::from(["users"]);
            client.reset_queries(&QueryKeyFilter::Prefix(&prefix), cx);

            assert!(u1.read(cx).data().is_none(), "users/1 should be reset");
            assert_eq!(u1.read(cx).status(), QueryStatus::Idle);
            assert!(
                p1.read(cx).data().is_some(),
                "posts/1 should keep its data"
            );
        });
    });
}

// ── 6. GC evicts stale Idle/Failure/Success resources ───────────────────
//
// GC uses a cached StatusSnapshot (not the live entity). The snapshot is
// updated by the hook layer in production. For deterministic tests, we use
// `client.update_query_snapshot()` to set the snapshot to a known state
// before calling `gc_with_time()`, then assert the expected outcome
// unconditionally.
//
// gc_time_ms=1000 means: MIN_GC_TIME_MS=1000 (enforced floor), so
//   - Idle/Failure: evicted when age >= gc_threshold (1000ms)
//   - Success: evicted when age >= success_threshold (2 * 1000 = 2000ms)
//   - Loading: never evicted (regardless of age)
//   - No snapshot (last_updated_ms=None): age defaults to gc_threshold, evicted
//
// NOTE: The tests below cover the basic eviction paths (idle with no snapshot,
// Failure/Success with snapshots, Loading preserved). For more comprehensive
// GC coverage including snapshot-bearing resources in all statuses with varied
// cache policies and edge-case timing, see `coverage_gaps.rs`.

#[gpui::test]
fn test_gc_evicts_idle_resources_with_no_snapshot(cx: &mut TestAppContext) {
    // Resources that have never been fetched (Idle, no snapshot update) are
    // evicted by GC since last_updated_ms=None is treated as expired.
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _entity = client.resource::<String, QueryError>("idle_key", cx);
            assert_eq!(client.all_queries::<String, QueryError>().len(), 1);

            // GC immediately — Idle with no snapshot is treated as expired
            client.gc_with_time(1_500, cx);

            let queries = client.all_queries::<String, QueryError>();
            assert!(
                queries.is_empty(),
                "idle resource with no snapshot should be evicted"
            );
        });
    });
}

#[gpui::test]
fn test_gc_evicts_failure_resources_after_gc_time(cx: &mut TestAppContext) {
    // A Failure resource whose snapshot age exceeds gc_time_ms MUST be evicted.
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("fail_key");

            // Create resource, fail a fetch, then update the snapshot to Failure
            let prepared = client
                .prepare_fetch_query::<String, QueryError>(key.clone(), cx)
                .expect("should start");
            prepared.complete_failure(QueryError::response("broken"), cx);

            // Simulate hook-layer snapshot update: Failure at t=1000
            client.update_query_snapshot::<String, QueryError>(
                &key,
                QueryStatus::Failure,
                Some(1_000),
                CachePolicy::Ttl { ttl_ms: 5_000 },
            );

            // GC at t=2500: age = 2500 - 1000 = 1500 > gc_threshold(1000) -> evicted
            client.gc_with_time(2_500, cx);

            assert!(
                client.query::<String, QueryError>(&key).is_none(),
                "failure resource should be evicted when age (1500ms) exceeds gc_time (1000ms)"
            );
        });
    });
}

#[gpui::test]
fn test_gc_preserves_failure_resources_before_gc_time(cx: &mut TestAppContext) {
    // A Failure resource whose snapshot age is within gc_time_ms MUST survive GC.
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("fail_key_early");

            // Create resource, fail a fetch, then update the snapshot to Failure
            let prepared = client
                .prepare_fetch_query::<String, QueryError>(key.clone(), cx)
                .expect("should start");
            prepared.complete_failure(QueryError::response("broken"), cx);

            // Simulate hook-layer snapshot update: Failure at t=2000
            client.update_query_snapshot::<String, QueryError>(
                &key,
                QueryStatus::Failure,
                Some(2_000),
                CachePolicy::Ttl { ttl_ms: 5_000 },
            );

            // GC at t=2500: age = 2500 - 2000 = 500 < gc_threshold(1000) -> preserved
            client.gc_with_time(2_500, cx);

            let entity = client
                .query::<String, QueryError>(&key)
                .expect("failure resource should survive when age (500ms) < gc_time (1000ms)");
            assert_eq!(
                entity.read(cx).status(),
                QueryStatus::Failure,
                "entity should still be in Failure state"
            );
        });
    });
}

#[gpui::test]
fn test_gc_preserves_loading_resources_regardless_of_age(cx: &mut TestAppContext) {
    // A Loading resource MUST survive GC even when its age far exceeds gc_time.
    // This tests the GC's "never evict loading" invariant through the public API.
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("loading_key");

            // Start a fetch via the public API but don't complete it
            let prepared = client
                .prepare_fetch_query::<String, QueryError>(key.clone(), cx)
                .expect("should start");

            // Simulate hook-layer snapshot update: LoadingEmpty at t=0
            client.update_query_snapshot::<String, QueryError>(
                &key,
                QueryStatus::LoadingEmpty,
                Some(0),
                CachePolicy::Ttl { ttl_ms: 5_000 },
            );

            // GC at t=1_000_000 — age is enormous, but Loading resources are never evicted
            client.gc_with_time(1_000_000, cx);

            assert!(
                client.query::<String, QueryError>(&key).is_some(),
                "loading resource must survive GC regardless of age"
            );

            // Complete the fetch via the public API to verify it still works
            prepared.complete_success("data".to_string(), cx);

            let entity = client
                .query::<String, QueryError>(&key)
                .expect("entity should still exist after completion");
            assert_eq!(
                entity.read(cx).status(),
                QueryStatus::Success,
                "entity should be Success after completing the fetch"
            );
            assert_eq!(
                entity.read(cx).data().unwrap(),
                "data",
                "data should match what was completed"
            );
        });
    });
}

#[gpui::test]
fn test_gc_evicts_success_resources_after_success_threshold(cx: &mut TestAppContext) {
    // A Success resource whose snapshot age exceeds SUCCESS_GC_MULTIPLIER * gc_time
    // (2 * 1000 = 2000ms) MUST be evicted.
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("success_old");

            let prepared = client
                .prepare_fetch_query::<String, QueryError>(key.clone(), cx)
                .expect("should start");
            prepared.complete_success("data".to_string(), cx);

            // Simulate hook-layer snapshot update: Success at t=1000
            client.update_query_snapshot::<String, QueryError>(
                &key,
                QueryStatus::Success,
                Some(1_000),
                CachePolicy::Ttl { ttl_ms: 5_000 },
            );

            // GC at t=3500: age = 3500 - 1000 = 2500 > success_threshold(2000) -> evicted
            client.gc_with_time(3_500, cx);

            assert!(
                client.query::<String, QueryError>(&key).is_none(),
                "success resource should be evicted when age (2500ms) exceeds success_threshold (2000ms)"
            );
        });
    });
}

#[gpui::test]
fn test_gc_preserves_success_resources_within_success_threshold(cx: &mut TestAppContext) {
    // A Success resource whose snapshot age is within SUCCESS_GC_MULTIPLIER * gc_time
    // (2 * 1000 = 2000ms) MUST survive GC.
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("success_fresh");

            let prepared = client
                .prepare_fetch_query::<String, QueryError>(key.clone(), cx)
                .expect("should start");
            prepared.complete_success("data".to_string(), cx);

            // Simulate hook-layer snapshot update: Success at t=2000
            client.update_query_snapshot::<String, QueryError>(
                &key,
                QueryStatus::Success,
                Some(2_000),
                CachePolicy::Ttl { ttl_ms: 5_000 },
            );

            // GC at t=3500: age = 3500 - 2000 = 1500 < success_threshold(2000) -> preserved
            client.gc_with_time(3_500, cx);

            let entity = client
                .query::<String, QueryError>(&key)
                .expect("success resource should survive when age (1500ms) < success_threshold (2000ms)");
            assert_eq!(
                entity.read(cx).data().unwrap(),
                "data",
                "data should be intact after GC"
            );
        });
    });
}

#[gpui::test]
fn test_gc_across_multiple_type_buckets(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create resources of different types
            let _s = client.resource::<String, QueryError>("s", cx);
            let _n = client.resource::<u32, QueryError>("n", cx);

            assert_eq!(client.all_queries::<String, QueryError>().len(), 1);
            assert_eq!(client.all_queries::<u32, QueryError>().len(), 1);

            // GC — idle resources with no snapshot will be evicted
            // (last_updated_ms=None is treated as age >= gc_threshold)
            client.gc_with_time(3_000, cx);

            assert!(
                client.all_queries::<String, QueryError>().is_empty(),
                "idle String resource evicted"
            );
            assert!(
                client.all_queries::<u32, QueryError>().is_empty(),
                "idle u32 resource evicted"
            );
        });
    });
}

// ── 7. Mutation lifecycle through QueryClient ──────────────────────────

#[gpui::test]
fn test_mutation_lifecycle_through_client(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = cx.new(|_| {
                MutationResource::<String, User, QueryError>::new(RetryPolicy::no_retries())
            });

            client.register_mutation::<String, User, QueryError>(&entity, cx);

            // Verify registration
            let mutations = client.all_mutations::<String, User, QueryError>();
            assert_eq!(mutations.len(), 1, "should have one registered mutation");

            // Begin mutation
            entity.update(cx, |m, _| {
                m.begin("new_name".to_string());
            });
            assert!(entity.read(cx).is_loading());
            assert_eq!(
                entity.read(cx).variables(),
                Some(&"new_name".to_string())
            );

            // Complete with success
            entity.update(cx, |m, _| {
                m.complete_success(User::new(1, "Alice Updated"));
            });
            assert!(entity.read(cx).is_success());
            assert_eq!(entity.read(cx).data().unwrap().name, "Alice Updated");
        });
    });
}

#[gpui::test]
fn test_mutation_failure_and_retry(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = cx.new(|_| {
                MutationResource::<String, User, QueryError>::new(RetryPolicy::new(2))
            });
            client.register_mutation::<String, User, QueryError>(&entity, cx);

            // Begin and fail first attempt
            entity.update(cx, |m, _| {
                m.begin("vars".to_string());
                m.complete_failure(QueryError::response("timeout"));
            });
            assert!(entity.read(cx).is_failure());
            assert_eq!(entity.read(cx).retry_count(), 1);

            // Retry
            entity.update(cx, |m, _| {
                assert!(m.retry());
            });
            assert!(entity.read(cx).is_loading());

            // Fail again — retries exhausted
            entity.update(cx, |m, _| {
                m.complete_failure(QueryError::response("timeout again"));
            });
            assert_eq!(entity.read(cx).retry_count(), 2);
            assert!(
                !entity.read(cx).should_retry(),
                "retries should be exhausted"
            );
        });
    });
}

#[gpui::test]
fn test_mutation_reset_clears_state(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        let entity = cx.new(|_| {
            MutationResource::<String, User, QueryError>::new(RetryPolicy::no_retries())
        });
        entity.update(cx, |m, _| {
            m.begin("vars".to_string());
            m.complete_success(User::default());
        });
        assert!(entity.read(cx).is_success());

        entity.update(cx, |m, _| m.reset());
        assert!(entity.read(cx).is_idle());
        assert!(entity.read(cx).data().is_none());
    });
}

// ── 8. Diagnostics output ──────────────────────────────────────────────

#[gpui::test]
fn test_diagnostics_empty_client(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        let diag = cx.update_global::<QueryClient, _>(|client, cx| client.diagnostics(cx));
        assert_eq!(diag.query_count, 0);
        assert_eq!(diag.mutation_count, 0);
        assert!(diag.queries.is_empty());
        assert!(diag.mutations.is_empty());
    });
}

#[gpui::test]
fn test_diagnostics_with_resources(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _e1 = client.resource::<String, QueryError>(
                QueryKey::from(["users", "1"]),
                cx,
            );

            // Use prepare_fetch_query + complete_success for a proper lifecycle
            let prepared = client
                .prepare_fetch_query::<String, QueryError>(
                    QueryKey::from(["users", "2"]),
                    cx,
                )
                .expect("should start");
            prepared.complete_success("Bob".to_string(), cx);

            let mutation = cx.new(|_| {
                MutationResource::<String, String, QueryError>::new(RetryPolicy::no_retries())
            });
            client.register_mutation::<String, String, QueryError>(&mutation, cx);

            let diag = client.diagnostics(cx);
            assert_eq!(diag.query_count, 2, "should have two query entries");
            assert_eq!(diag.mutation_count, 1, "should have one mutation");
            assert_eq!(
                diag.queries.len(),
                2,
                "diagnostics should have two query records"
            );

            // Verify that the completed query shows up in diagnostics.
            // Note: diagnostics reads live entity state via collect_diagnostics,
            // which upgrades weak refs and reads entity state.
            let users_diags: Vec<_> = diag
                .queries
                .iter()
                .filter(|q| q.key.contains("users"))
                .collect();
            assert_eq!(users_diags.len(), 2, "should have two user queries");

            // The entity that was completed via PreparedFetch should show Success
            let success_diag = users_diags
                .iter()
                .find(|q| q.status == QueryStatus::Success);
            assert!(
                success_diag.is_some(),
                "at least one query should show Success (the one completed via PreparedFetch)"
            );
        });
    });
}

#[gpui::test]
fn test_diagnostics_across_type_buckets(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _s = client.resource::<String, QueryError>("s", cx);
            let _u = client.resource::<User, QueryError>("u", cx);

            let diag = client.diagnostics(cx);
            assert_eq!(diag.query_count, 2, "should count across type buckets");
            assert_eq!(diag.queries.len(), 2);
        });
    });
}

// ── 9. Signal retrieval and cancellation ───────────────────────────────

#[gpui::test]
fn test_cancel_queries_cancels_loading_requests(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("cancel_target");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);

            // Start a request
            let request_id = client
                .next_request_id_for_key::<String, QueryError>(&key)
                .expect("should get request id");
            entity.update(cx, |r, _| {
                r.begin_request_with_id(
                    Some(request_id),
                    1_000,
                    QueryFetchMode::Normal,
                );
            });
            assert!(entity.read(cx).is_loading());

            // Grab the signal before cancelling
            let signal = entity
                .read(cx)
                .signal()
                .expect("signal should exist while loading")
                .clone();
            assert!(!signal.is_cancelled());

            // Cancel via client bulk operation
            client.cancel_queries(&QueryKeyFilter::Exact(&key), cx);

            assert!(
                signal.is_cancelled(),
                "signal should be cancelled after cancel_queries"
            );
        });
    });
}

#[gpui::test]
fn test_cancel_queries_skips_idle_resources(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("idle_key");
            let _entity = client.resource::<String, QueryError>(key.clone(), cx);

            // Resource is idle — cancel_queries should be a no-op
            client.cancel_queries(&QueryKeyFilter::Exact(&key), cx);

            let entity = client.query::<String, QueryError>(&key);
            assert!(entity.is_some(), "resource should still exist");
            assert_eq!(
                entity.unwrap().read(cx).status(),
                QueryStatus::Idle,
                "status should remain Idle"
            );
        });
    });
}

#[gpui::test]
fn test_remove_queries_removes_matching(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _u1 = client.resource::<String, QueryError>(
                QueryKey::from(["users", "1"]),
                cx,
            );
            let _u2 = client.resource::<String, QueryError>(
                QueryKey::from(["users", "2"]),
                cx,
            );
            let _p1 = client.resource::<String, QueryError>(
                QueryKey::from(["posts", "1"]),
                cx,
            );

            assert_eq!(client.all_queries::<String, QueryError>().len(), 3);

            let prefix = QueryKey::from(["users"]);
            client.remove_queries(&QueryKeyFilter::Prefix(&prefix));

            let remaining = client.all_queries::<String, QueryError>();
            assert_eq!(remaining.len(), 1, "only the post should remain");
            assert!(
                client
                    .query::<String, QueryError>(&QueryKey::from(["posts", "1"]))
                    .is_some(),
                "post should still exist"
            );
            assert!(
                client
                    .query::<String, QueryError>(&QueryKey::from(["users", "1"]))
                    .is_none(),
                "user:1 should be removed"
            );
        });
    });
}

// ── 10. set_query_data / rollback_query_data ───────────────────────────

#[gpui::test]
fn test_set_query_data_sets_data_on_existing_resource(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("user:1");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);

            // Populate with real data
            entity.update(cx, |r, _| {
                r.apply_success("Alice".to_string(), 1_000);
            });
            assert_eq!(entity.read(cx).data().unwrap(), "Alice");

            // Overwrite via set_query_data
            client.set_query_data::<String, QueryError>(
                "user:1",
                "Bob".to_string(),
                cx,
            );

            assert_eq!(entity.read(cx).data().unwrap(), "Bob");
            assert_eq!(
                entity.read(cx).previous_data().unwrap(),
                "Alice",
                "previous_data should hold the pre-set value"
            );
        });
    });
}

#[gpui::test]
fn test_set_query_data_creates_resource_if_missing(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // set_query_data on a key that does not exist yet
            client.set_query_data::<String, QueryError>(
                "new_key",
                "hello".to_string(),
                cx,
            );

            let data = client.get_query_data::<String, QueryError>(
                &QueryKey::from("new_key"),
                cx,
            );
            assert_eq!(data, Some("hello".to_string()));
        });
    });
}

#[gpui::test]
fn test_get_query_data_returns_none_for_missing_key(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let data = client.get_query_data::<String, QueryError>(
                &QueryKey::from("ghost"),
                cx,
            );
            assert!(data.is_none(), "should return None for nonexistent key");
        });
    });
}

#[gpui::test]
fn test_rollback_query_data_via_resource(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("user:1");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);

            // Populate then optimistic update
            entity.update(cx, |r, _| r.apply_success("Alice".to_string(), 1_000));
            client.set_query_data::<String, QueryError>(
                "user:1",
                "Alice (optimistic)".to_string(),
                cx,
            );
            assert_eq!(
                entity.read(cx).data().unwrap(),
                "Alice (optimistic)"
            );

            // Rollback via the resource directly
            let rolled_back = entity.update(cx, |r, _| r.rollback_to_previous());
            assert!(rolled_back);
            assert_eq!(entity.read(cx).data().unwrap(), "Alice");
            assert_eq!(entity.read(cx).status(), QueryStatus::Success);
        });
    });
}

// ── 11. Observer creation and notification ─────────────────────────────

#[gpui::test]
fn test_query_observer_creation(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = client.resource::<String, QueryError>("obs_key", cx);
            let _observer = QueryObserver::new(&entity);
            // Observer created successfully — entity is still alive
            let weak = entity.downgrade();
            assert!(weak.upgrade().is_some(), "entity should still be alive");
        });
    });
}

#[gpui::test]
fn test_query_observer_observe_returns_subscription(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = client.resource::<String, QueryError>("sub_key", cx);

            // Create a dummy view to host the observer
            struct DummyView;
            let view = cx.new(|_| DummyView);

            let mut observer = QueryObserver::new(&entity);
            let subscription = view.update(cx, |_view, cx| observer.observe(cx));
            assert!(
                subscription.is_some(),
                "observe should return Some(Subscription)"
            );

            drop(subscription);
        });
    });
}

#[gpui::test]
fn test_mutation_observer_creation(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        let entity = cx.new(|_| {
            MutationResource::<String, User, QueryError>::new(RetryPolicy::no_retries())
        });
        let _observer = MutationObserver::<String, User, QueryError>::new(&entity);
        // No panic — observer created
    });
}

#[gpui::test]
fn test_observer_config_custom_settings(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = client.resource::<String, QueryError>("config_key", cx);
            let config = ObserverConfig {
                notify_on_status_change_only: false,
            };
            let _observer = QueryObserver::new(&entity).with_config(config);
            // Observer created with custom config — no panic
        });
    });
}

// ── 12. Full lifecycle: Idle -> Loading -> Success -> GC ──────────────

#[gpui::test]
fn test_full_lifecycle_idle_to_loading_to_success_to_gc(cx: &mut TestAppContext) {
    // Uses gc_time=5000ms. After completing a fetch and updating the snapshot
    // to Success at t=1000, GC at t=2800 produces age=1800 which is within
    // success_threshold (2*5000=10000ms), so the resource MUST survive.
    setup_query_client_with_gc(cx, 5_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // 1. Start fetch via the public API
            let key = QueryKey::from(["users", "42"]);
            let prepared = client
                .prepare_fetch_query::<String, QueryError>(key.clone(), cx)
                .expect("should start fetch");

            let entity = client
                .query::<String, QueryError>(&key)
                .expect("entity should exist");
            assert!(entity.read(cx).is_loading());

            // 2. Complete with success
            prepared.complete_success("Carol".to_string(), cx);
            assert_eq!(entity.read(cx).status(), QueryStatus::Success);
            assert_eq!(entity.read(cx).data().unwrap(), "Carol");

            // 3. Simulate hook-layer snapshot update: Success at t=1000
            client.update_query_snapshot::<String, QueryError>(
                &key,
                QueryStatus::Success,
                Some(1_000),
                CachePolicy::Ttl { ttl_ms: 5_000 },
            );

            // 4. GC at t=2800: age = 2800 - 1000 = 1800 < success_threshold(10000) -> preserved
            client.gc_with_time(2_800, cx);

            // 5. Unconditional assertion: the resource MUST survive GC
            let surviving = client
                .query::<String, QueryError>(&key)
                .expect("success resource should survive GC (age 1800ms < success_threshold 10000ms)");
            assert_eq!(
                surviving.read(cx).data().unwrap(),
                "Carol",
                "data should be intact after GC"
            );
        });
    });
}

#[gpui::test]
fn test_full_lifecycle_failure_recovery(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("flaky");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);

            // Start and fail
            let rid1 = client
                .next_request_id_for_key::<String, QueryError>(&key)
                .expect("request id");
            entity.update(cx, |r, _| {
                r.begin_request_with_id(Some(rid1), 1_000, QueryFetchMode::Normal);
            });
            entity.update(cx, |r, _| {
                r.complete_current_failure(rid1, QueryError::response("fail"), 1_100)
            });
            assert_eq!(entity.read(cx).status(), QueryStatus::Failure);
            assert!(entity.read(cx).data().is_none());

            // Retry and succeed
            let rid2 = client
                .next_request_id_for_key::<String, QueryError>(&key)
                .expect("request id 2");
            entity.update(cx, |r, _| {
                r.begin_request_with_id(Some(rid2), 1_200, QueryFetchMode::Normal);
            });
            entity.update(cx, |r, _| {
                r.complete_current_success(rid2, "recovered".to_string(), 1_300)
            });
            assert_eq!(entity.read(cx).status(), QueryStatus::Success);
            assert_eq!(entity.read(cx).data().unwrap(), "recovered");
        });
    });
}

// ── 13. Optimistic update full lifecycle ───────────────────────────────

#[gpui::test]
fn test_optimistic_update_and_rollback_lifecycle(cx: &mut TestAppContext) {
    // Use NoCache policy so begin_request always starts a new fetch
    setup_query_client_with_policies(cx, CachePolicy::NoCache, RequestPolicy::LatestWins);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from(["users", "42"]);
            let entity = client.resource::<String, QueryError>(key.clone(), cx);

            // 1. Populate with real data via direct apply
            entity.update(cx, |r, _| r.apply_success("Carol".to_string(), 1_000));

            // 2. Optimistic update
            client.set_query_data::<String, QueryError>(
                key.clone(),
                "Carol (saving...)".to_string(),
                cx,
            );
            assert_eq!(entity.read(cx).data().unwrap(), "Carol (saving...)");
            assert_eq!(entity.read(cx).previous_data().unwrap(), "Carol");

            // 3. Start mutation request (Force mode to bypass cache)
            let rid = client
                .next_request_id_for_key::<String, QueryError>(&key)
                .expect("request id");
            entity.update(cx, |r, _| {
                r.begin_request_with_id(Some(rid), 1_100, QueryFetchMode::Force);
            });
            assert!(entity.read(cx).is_loading());

            // 4. Mutation succeeds with real server data
            entity.update(cx, |r, _| {
                r.complete_current_success(rid, "Carol (saved)".to_string(), 1_200)
            });
            assert_eq!(entity.read(cx).status(), QueryStatus::Success);
            assert_eq!(entity.read(cx).data().unwrap(), "Carol (saved)");
        });
    });
}

#[gpui::test]
fn test_optimistic_update_rollback_on_failure(cx: &mut TestAppContext) {
    setup_query_client_with_policies(cx, CachePolicy::NoCache, RequestPolicy::LatestWins);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from(["users", "42"]);
            let entity = client.resource::<String, QueryError>(key.clone(), cx);

            // 1. Populate
            entity.update(cx, |r, _| r.apply_success("Carol".to_string(), 1_000));

            // 2. Optimistic update
            client.set_query_data::<String, QueryError>(
                key.clone(),
                "Carol (saving...)".to_string(),
                cx,
            );

            // 3. Start and fail (Force mode to bypass cache)
            let rid = client
                .next_request_id_for_key::<String, QueryError>(&key)
                .expect("request id");
            entity.update(cx, |r, _| {
                r.begin_request_with_id(Some(rid), 1_100, QueryFetchMode::Force);
            });
            entity.update(cx, |r, _| {
                r.complete_current_failure(
                    rid,
                    QueryError::response("network error"),
                    1_200,
                )
            });

            assert_eq!(entity.read(cx).status(), QueryStatus::Failure);

            // 4. Rollback
            let rolled_back = entity.update(cx, |r, _| r.rollback_to_previous());
            assert!(rolled_back);
            assert_eq!(entity.read(cx).data().unwrap(), "Carol");
            assert_eq!(entity.read(cx).status(), QueryStatus::Success);
        });
    });
}

// ── 14. Dehydrate / hydrate ────────────────────────────────────────────

#[gpui::test]
fn test_dehydrate_collects_success_entries(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Success resource — should appear in dehydrate
            let e1 = client.resource::<String, QueryError>(QueryKey::from("ok"), cx);
            e1.update(cx, |r, _| r.apply_success("data".to_string(), 1_000));

            // Idle resource — should NOT appear
            let _e2 =
                client.resource::<String, QueryError>(QueryKey::from("idle"), cx);

            let state = client.dehydrate(cx);
            assert_eq!(
                state.entries.len(),
                1,
                "only Success resources should be dehydrated"
            );
            assert_eq!(state.entries[0].key, "ok");
            assert_eq!(state.entries[0].kind, "query");
        });
    });
}

#[gpui::test]
fn test_dehydrate_skips_non_success_resources(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Failure resource
            let e1 = client.resource::<String, QueryError>("fail", cx);
            e1.update(cx, |r, _| {
                r.apply_failure(QueryError::response("err"), 1_000)
            });

            // Idle (no data)
            let _e2 = client.resource::<String, QueryError>("idle2", cx);

            let state = client.dehydrate(cx);
            assert!(
                state.entries.is_empty(),
                "non-Success resources should not be dehydrated"
            );
        });
    });
}

// ── 15. next_request_id_for_key monotonic sequence ─────────────────────

#[gpui::test]
fn test_next_request_id_is_monotonically_increasing(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("seq_test");
            let _entity = client.resource::<String, QueryError>(key.clone(), cx);

            let id1 = client.next_request_id_for_key::<String, QueryError>(&key);
            let id2 = client.next_request_id_for_key::<String, QueryError>(&key);
            let id3 = client.next_request_id_for_key::<String, QueryError>(&key);

            assert!(id1.is_some());
            assert!(id2.is_some());
            assert!(id3.is_some());

            let id1 = id1.unwrap();
            let id2 = id2.unwrap();
            let id3 = id3.unwrap();

            assert!(id1.value() < id2.value(), "sequence should be increasing");
            assert!(id2.value() < id3.value(), "sequence should be increasing");
            assert_eq!(id1.scope_id(), id2.scope_id(), "same scope for same key");
        });
    });
}

#[gpui::test]
fn test_next_request_id_returns_none_for_missing_key(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, _cx| {
            let id = client.next_request_id_for_key::<String, QueryError>(
                &QueryKey::from("ghost"),
            );
            assert!(id.is_none(), "should return None for nonexistent key");
        });
    });
}

// ── 16. PreparedFetch lifecycle ────────────────────────────────────────

#[gpui::test]
fn test_prepare_fetch_query_success_lifecycle(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("fetch_key");
            let prepared = client.prepare_fetch_query::<String, QueryError>(key.clone(), cx);

            assert!(prepared.is_some(), "should return Some for new resource");
            let prepared = prepared.unwrap();
            assert!(
                !prepared.signal.is_cancelled(),
                "signal should start uncancelled"
            );

            // Complete with success
            prepared.complete_success("fetched_data".to_string(), cx);

            // Verify data is stored
            let data = client.get_query_data::<String, QueryError>(&key, cx);
            assert_eq!(data, Some("fetched_data".to_string()));
        });
    });
}

#[gpui::test]
fn test_prepare_fetch_query_failure_lifecycle(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("fail_fetch");
            let prepared = client.prepare_fetch_query::<String, QueryError>(key.clone(), cx);
            assert!(prepared.is_some());

            let prepared = prepared.unwrap();
            prepared.complete_failure(QueryError::response("server error"), cx);

            let entity = client.query::<String, QueryError>(&key).unwrap();
            assert_eq!(entity.read(cx).status(), QueryStatus::Failure);
        });
    });
}

// ── 17. prepare_prefetch_query ─────────────────────────────────────────

#[gpui::test]
fn test_prepare_prefetch_query_starts_for_stale_resource(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("prefetch_key");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);

            // Populate with stale data (old timestamp)
            entity.update(cx, |r, _| r.apply_success("old_data".to_string(), 100));

            // Prefetch should start since data is stale at t=5000 with TTL=1000
            let prepared = client.prepare_prefetch_query::<String, QueryError>(
                key.clone(),
                CachePolicy::Ttl { ttl_ms: 1_000 },
                RequestPolicy::LatestWins,
                cx,
            );
            assert!(
                prepared.is_some(),
                "prefetch should start for stale resource"
            );

            let prepared = prepared.unwrap();
            prepared.complete_success("fresh_data".to_string(), cx);

            let data = client.get_query_data::<String, QueryError>(&key, cx);
            assert_eq!(data, Some("fresh_data".to_string()));
        });
    });
}

// ── 18. Persistence trait integration ──────────────────────────────────

#[gpui::test]
fn test_persist_and_restore_cycle(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create a success resource
            let entity = client.resource::<String, QueryError>("persist_me", cx);
            entity.update(cx, |r, _| r.apply_success("value".to_string(), 1_000));

            // Mock persister backed by in-memory storage
            struct MemPersister {
                entries: Mutex<Vec<crate::client::DehydratedEntry>>,
            }
            impl crate::client::QueryPersister for MemPersister {
                fn load(&self) -> Vec<crate::client::DehydratedEntry> {
                    self.entries.lock().unwrap().clone()
                }
                fn save(&self, entries: Vec<crate::client::DehydratedEntry>) {
                    *self.entries.lock().unwrap() = entries;
                }
            }

            let persister = MemPersister {
                entries: Mutex::new(Vec::new()),
            };

            // Persist
            client.persist(&persister, cx);

            // Restore
            let loaded = client.restore(&persister);
            assert_eq!(loaded.len(), 1, "should have one persisted entry");
            assert_eq!(loaded[0].key, "persist_me");
        });
    });
}
