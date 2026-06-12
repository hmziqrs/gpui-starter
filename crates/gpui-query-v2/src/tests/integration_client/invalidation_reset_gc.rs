//! Tests for cache management: invalidation, reset, and garbage collection.

use gpui::{AppContext as _, BorrowAppContext as _, TestAppContext};

use crate::client::QueryClient;
use crate::core::*;
use crate::tests::test_support::*;

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
            assert!(e2.read(cx).is_cache_fresh(1_500), "e2 should remain fresh");
        });
    });
}

#[gpui::test]
fn test_invalidate_queries_prefix_filter_across_types(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let u1 = client.resource::<String, QueryError>(QueryKey::from(["users", "1"]), cx);
            let u2 = client.resource::<String, QueryError>(QueryKey::from(["users", "2"]), cx);
            let p1 = client.resource::<String, QueryError>(QueryKey::from(["posts", "1"]), cx);

            u1.update(cx, |r, _| r.apply_success("user1".to_string(), 1_000));
            u2.update(cx, |r, _| r.apply_success("user2".to_string(), 1_000));
            p1.update(cx, |r, _| r.apply_success("post1".to_string(), 1_000));

            // Invalidate all "users" — posts unaffected
            let prefix = QueryKey::from(["users"]);
            client.invalidate_queries(&QueryKeyFilter::Prefix(&prefix), cx);

            assert!(
                !u1.read(cx).is_cache_fresh(1_500),
                "users/1 should be stale"
            );
            assert!(
                !u2.read(cx).is_cache_fresh(1_500),
                "users/2 should be stale"
            );
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
            let u1 = client.resource::<String, QueryError>(QueryKey::from(["users", "1"]), cx);
            let p1 = client.resource::<String, QueryError>(QueryKey::from(["posts", "1"]), cx);

            u1.update(cx, |r, _| r.apply_success("user".to_string(), 1_000));
            p1.update(cx, |r, _| r.apply_success("post".to_string(), 1_000));

            let prefix = QueryKey::from(["users"]);
            client.reset_queries(&QueryKeyFilter::Prefix(&prefix), cx);

            assert!(u1.read(cx).data().is_none(), "users/1 should be reset");
            assert_eq!(u1.read(cx).status(), QueryStatus::Idle);
            assert!(p1.read(cx).data().is_some(), "posts/1 should keep its data");
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

            let entity = client.query::<String, QueryError>(&key).expect(
                "success resource should survive when age (1500ms) < success_threshold (2000ms)",
            );
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
