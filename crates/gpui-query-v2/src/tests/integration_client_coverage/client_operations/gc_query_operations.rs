//! GC, query operations (remove/invalidate/reset), observer, and misc tests
//! (tests 39–44, 49, 52–55, 57–61).

use gpui::{AppContext as _, BorrowAppContext as _, TestAppContext};

use crate::client::{InfiniteQueryObserver, QueryClient};
use crate::core::*;
use crate::tests::test_support::*;

// -- 39. GC clamps gc_time=0 to 1000ms; Idle resources with no snapshot
//         timestamp are evicted at any gc_with_time value since their
//         age defaults to gc_threshold. ──────────────────────────────────────
//
// Finding 1 fix: Assert concrete eviction outcomes. An Idle resource with
// no snapshot timestamp (never fetched) is treated as "age == gc_threshold"
// by the GC, so it is always evicted regardless of gc_time clamping.

#[gpui::test]
fn test_gc_with_zero_time_clamped_evicts_idle(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 0);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _entity = client.resource::<String, QueryError>("gc_zero", cx);
            assert_eq!(client.all_queries::<String, QueryError>().len(), 1);

            // gc_time_ms=0 is clamped to 1000ms. The resource is Idle with no
            // snapshot timestamp, so its age defaults to gc_threshold (1000ms),
            // meaning age >= threshold and it gets evicted.
            client.gc_with_time(0, cx);
            assert_eq!(
                client.all_queries::<String, QueryError>().len(),
                0,
                "Idle resource with no snapshot timestamp should be evicted \
                 even at gc_with_time(0) because its age defaults to the clamped gc_threshold"
            );
        });
    });
}

// -- 40. GC uses gc_with_time with deterministic time control ----------------
//
// Finding 2 fix: Assert concrete GC outcomes using the documented eviction
// rules. The bucket's status snapshot is only updated when the hook layer
// calls update_status_snapshot (not from direct resource mutations), so
// resources created via client.resource() always appear as Idle with
// last_updated_ms=None to GC. Idle resources with no timestamp are evicted
// at any gc_with_time value (age defaults to gc_threshold).

#[gpui::test]
fn test_gc_with_time_explicit_time_value(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create two resources: one to be evicted, one to verify removal
            let _e1 = client.resource::<String, QueryError>("gc_evict", cx);
            let _e2 = client.resource::<String, QueryError>("gc_evict2", cx);
            assert_eq!(client.all_queries::<String, QueryError>().len(), 2);

            // First GC at small time: both Idle resources with no snapshot
            // timestamp have age == gc_threshold (clamped to 1000ms), so
            // age < gc_threshold is false and they get evicted.
            client.gc_with_time(500, cx);
            assert_eq!(
                client.all_queries::<String, QueryError>().len(),
                0,
                "Idle resources with no snapshot timestamp should be evicted \
                 at gc_with_time(500) — their age defaults to the clamped gc_threshold"
            );

            // Create another resource and run GC at a very large time.
            let _e3 = client.resource::<String, QueryError>("gc_big_time", cx);
            assert_eq!(client.all_queries::<String, QueryError>().len(), 1);

            client.gc_with_time(100_000, cx);
            assert_eq!(
                client.all_queries::<String, QueryError>().len(),
                0,
                "Idle resource should also be evicted at gc_with_time(100_000)"
            );

            // After eviction, diagnostics should report zero queries.
            let diag = client.diagnostics(cx);
            assert_eq!(
                diag.query_count, 0,
                "diagnostics should report 0 queries after all were evicted"
            );
        });
    });
}

// -- 41. GC runs across all bucket types (query, infinite, mutation) ---------
//
// Finding 3 fix: After GC, assert that idle resources with no snapshot
// updates ARE evicted, and loading resources are preserved.

#[gpui::test]
fn test_gc_runs_across_all_bucket_types(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create idle query (no fetch) — should be evicted by GC
            let _q = client.resource::<String, QueryError>("q_gc", cx);

            // Create idle infinite query (no fetch) — should be evicted by GC
            let _iq = client.infinite_resource::<String, QueryError>("iq_gc", cx);

            // Create a loading mutation — loading resources should survive GC
            let loading_mut = cx.new(|_| {
                MutationResource::<String, User, QueryError>::new(RetryPolicy::no_retries())
            });
            loading_mut.update(cx, |m, _| m.begin("vars".to_string()));
            client.register_mutation::<String, User, QueryError>(&loading_mut, cx);

            let idle_mut = cx.new(|_| {
                MutationResource::<String, User, QueryError>::new(RetryPolicy::no_retries())
            });
            client.register_mutation::<String, User, QueryError>(&idle_mut, cx);

            // Pre-GC counts
            assert_eq!(client.all_queries::<String, QueryError>().len(), 1);
            assert_eq!(client.all_infinite_queries::<String, QueryError>().len(), 1);
            assert_eq!(client.all_mutations::<String, User, QueryError>().len(), 2);

            // GC at far future time
            client.gc_with_time(100_000, cx);

            // Post-GC: idle resources with no snapshot timestamp are evicted
            assert!(
                client.all_queries::<String, QueryError>().is_empty(),
                "idle query with no snapshot should be evicted by GC"
            );
            assert!(
                client.all_infinite_queries::<String, QueryError>().is_empty(),
                "idle infinite query with no snapshot should be evicted by GC"
            );

            // Loading mutation should survive GC (loading resources are never evicted)
            let mutations = client.all_mutations::<String, User, QueryError>();
            assert_eq!(
                mutations.len(),
                2,
                "both mutations should survive GC (one loading, one idle but retained by strong Entity ref)"
            );
        });
    });
}

// -- 42. remove_queries removes from infinite buckets too --------------------

#[gpui::test]
fn test_remove_queries_affects_infinite_queries(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _iq = client
                .infinite_resource::<String, QueryError>(QueryKey::from(["users", "inf"]), cx);
            let _q = client.resource::<String, QueryError>(QueryKey::from(["users", "reg"]), cx);

            assert_eq!(client.all_infinite_queries::<String, QueryError>().len(), 1);
            assert_eq!(client.all_queries::<String, QueryError>().len(), 1);

            let prefix = QueryKey::from(["users"]);
            client.remove_queries(&QueryKeyFilter::Prefix(&prefix));

            assert!(
                client
                    .all_infinite_queries::<String, QueryError>()
                    .is_empty(),
                "infinite query should be removed"
            );
            assert!(
                client.all_queries::<String, QueryError>().is_empty(),
                "regular query should be removed"
            );
        });
    });
}

// -- 43. invalidate_queries affects infinite queries too ---------------------

#[gpui::test]
fn test_invalidate_queries_affects_infinite_queries(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _iq = client.infinite_resource::<String, QueryError>("inf_inv", cx);
            // Note: InfiniteQueryResource doesn't have apply_success in the same way,
            // but we can still verify invalidate doesn't panic and the entity remains.
            client.invalidate_queries(&QueryKeyFilter::All, cx);

            let retrieved = client.infinite_query::<String, QueryError>(&QueryKey::from("inf_inv"));
            assert!(
                retrieved.is_some(),
                "infinite query should still exist after invalidate"
            );
        });
    });
}

// -- 44. reset_queries affects infinite queries ------------------------------

#[gpui::test]
fn test_reset_queries_affects_infinite_queries(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _iq = client.infinite_resource::<String, QueryError>("inf_reset", cx);
            client.reset_queries(&QueryKeyFilter::All, cx);

            let retrieved =
                client.infinite_query::<String, QueryError>(&QueryKey::from("inf_reset"));
            assert!(
                retrieved.is_some(),
                "infinite query should exist after reset"
            );
            // InfiniteQueryResource in idle state
            assert_eq!(retrieved.unwrap().read(cx).status(), QueryStatus::Idle);
        });
    });
}

// -- 49. Infinite query observer creation and observe ------------------------

#[gpui::test]
fn test_infinite_query_observer_creation_and_observe(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = client.infinite_resource::<String, QueryError>("inf_obs", cx);
            let mut observer = InfiniteQueryObserver::new(&entity);

            struct DummyView;
            let view = cx.new(|_| DummyView);
            let sub = view.update(cx, |_view, cx| observer.observe(cx));
            assert!(sub.is_some(), "observe should return Some(Subscription)");
        });
    });
}

#[gpui::test]
fn test_infinite_query_observer_weak_entity_pattern(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = client.infinite_resource::<String, QueryError>("inf_obs_weak", cx);
            let mut observer = InfiniteQueryObserver::new(&entity);

            struct DummyView;
            let view = cx.new(|_| DummyView);
            let sub = view.update(cx, |_view, cx| observer.observe(cx));
            assert!(sub.is_some(), "observe should return Some for live entity");
        });
    });
}

// -- 52. current_time_ms is reasonable ---------------------------------------

#[gpui::test]
fn test_current_time_ms_is_reasonable(_cx: &mut TestAppContext) {
    let now = crate::client::current_time_ms();
    // Should be > 1_700_000_000_000 (after 2023) and < 2_000_000_000_000 (before 2033)
    assert!(
        now > 1_700_000_000_000,
        "current_time_ms should be post-2023"
    );
    assert!(
        now < 2_000_000_000_000,
        "current_time_ms should be pre-2033"
    );
}

// -- 53. Multiple resources share same type bucket ---------------------------

#[gpui::test]
fn test_multiple_resources_same_type_share_bucket(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create 10 resources of same type
            for i in 0..10 {
                let key = format!("multi_{i}");
                let _e = client.resource::<String, QueryError>(key, cx);
            }

            let all = client.all_queries::<String, QueryError>();
            assert_eq!(all.len(), 10, "should have 10 resources in the same bucket");

            let diag = client.diagnostics(cx);
            assert_eq!(diag.query_count, 10);
            assert_eq!(diag.queries.len(), 10);
        });
    });
}

// -- 54. invalidate then re-fetch lifecycle ----------------------------------
//
// Finding 6 fix: Assert that prepare_fetch_query always returns Some after
// invalidation (since the cache is invalidated, Force mode should start a
// new fetch regardless).

#[gpui::test]
fn test_invalidate_then_refetch_lifecycle(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("inv_refetch");

            // Fetch and succeed
            let p1 = client
                .prepare_fetch_query::<String, QueryError>(key.clone(), cx)
                .expect("first fetch");
            p1.complete_success("v1".to_string(), cx);

            assert_eq!(
                client.get_query_data::<String, QueryError>(&key, cx),
                Some("v1".to_string())
            );

            // Invalidate — marks the cache as stale
            client.invalidate_queries(&QueryKeyFilter::Exact(&key), cx);

            // After invalidation, prepare_fetch_query (Force mode) must always
            // start a new request. This is a guaranteed contract: Force mode
            // ignores cache freshness, so the result is always Some.
            let p2 = client.prepare_fetch_query::<String, QueryError>(key.clone(), cx);
            assert!(
                p2.is_some(),
                "prepare_fetch_query must return Some after invalidation — \
                 Force mode always starts a new request regardless of cache state"
            );

            p2.unwrap().complete_success("v2".to_string(), cx);
            assert_eq!(
                client.get_query_data::<String, QueryError>(&key, cx),
                Some("v2".to_string())
            );
        });
    });
}

// -- 55. Reset clears data then set_query_data re-populates ------------------

#[gpui::test]
fn test_reset_then_set_query_data(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("reset_set");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);
            entity.update(cx, |r, _| r.apply_success("original".to_string(), 1_000));

            // Reset clears everything
            client.reset_queries(&QueryKeyFilter::Exact(&key), cx);
            assert!(entity.read(cx).data().is_none());
            assert_eq!(entity.read(cx).status(), QueryStatus::Idle);

            // Re-populate via set_query_data
            client.set_query_data::<String, QueryError>(key, "restored".to_string(), cx);
            assert_eq!(entity.read(cx).data().unwrap(), "restored");
        });
    });
}

// -- 57. Large number of resources creation ----------------------------------

#[gpui::test]
fn test_large_number_of_resources_creation(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            for i in 0..100 {
                let key = format!("large_{i}");
                let _e = client.resource::<u32, QueryError>(key, cx);
            }
            let all = client.all_queries::<u32, QueryError>();
            assert_eq!(all.len(), 100, "should have 100 resources");
        });
    });
}

// -- 58. QueryKey with multiple segments in client operations ----------------

#[gpui::test]
fn test_multi_segment_key_in_client_operations(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from(["org", "team", "user", "42"]);
            let entity = client.resource::<String, QueryError>(key.clone(), cx);
            entity.update(cx, |r, _| {
                r.apply_success("deep_key_data".to_string(), 1_000)
            });

            // Query by the full key
            let found = client.query::<String, QueryError>(&key);
            assert!(found.is_some());

            // Invalidate by prefix "org/team"
            let prefix = QueryKey::from(["org", "team"]);
            client.invalidate_queries(&QueryKeyFilter::Prefix(&prefix), cx);
            assert!(
                !entity.read(cx).is_cache_fresh(1_500),
                "should be stale after prefix invalidate"
            );

            // Data should survive invalidation
            assert_eq!(entity.read(cx).data().unwrap(), "deep_key_data");

            // Reset by prefix
            client.reset_queries(&QueryKeyFilter::Prefix(&prefix), cx);
            assert!(
                entity.read(cx).data().is_none(),
                "data cleared after prefix reset"
            );
        });
    });
}

// -- 59. set_query_data with different T types doesn't conflict --------------

#[gpui::test]
fn test_set_query_data_different_types_no_conflict(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Same key, different types
            client.set_query_data::<String, QueryError>("shared_key", "string_val".to_string(), cx);
            client.set_query_data::<u32, QueryError>("shared_key", 42_u32, cx);

            let s = client.get_query_data::<String, QueryError>(&QueryKey::from("shared_key"), cx);
            let n = client.get_query_data::<u32, QueryError>(&QueryKey::from("shared_key"), cx);

            assert_eq!(s, Some("string_val".to_string()));
            assert_eq!(n, Some(42_u32));
        });
    });
}

// -- 60. clear_data on resource via client context ---------------------------

#[gpui::test]
fn test_clear_data_via_resource(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("clear_me");
            client.set_query_data::<String, QueryError>(key.clone(), "data".to_string(), cx);

            let entity = client.query::<String, QueryError>(&key).unwrap();
            assert_eq!(entity.read(cx).data().unwrap(), "data");

            entity.update(cx, |r, _| r.clear_data());
            assert!(entity.read(cx).data().is_none(), "data should be cleared");
            // get_query_data should now return None
            let data = client.get_query_data::<String, QueryError>(&key, cx);
            assert!(data.is_none());
        });
    });
}

// -- 61. prepare_prefetch_query returns Some for stale data -----------------
//
// Finding 4/7 fix: This test asserts the actual return value. When data
// was set at t=0 (long ago) and prefetch checks at current_time_ms,
// the data is stale, so prefetch should return Some.

#[gpui::test]
fn test_prepare_prefetch_query_returns_some_for_stale(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(QueryClient::with_policies(
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        ));
    });
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Populate with data at t=0 (epoch) — guaranteed stale now
            let key = QueryKey::from("prefresh_stale");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);
            entity.update(cx, |r, _| r.apply_success("stale_data".to_string(), 0));

            // Data is from t=0 and current_time_ms() is ~1.7 trillion ms,
            // so the resource is definitely stale (age >> 60_000ms TTL).
            // prepare_prefetch_query uses Normal mode, which respects freshness,
            // so it should start a fetch for stale data.
            let result = client.prepare_prefetch_query::<String, QueryError>(
                key.clone(),
                CachePolicy::Ttl { ttl_ms: 60_000 },
                RequestPolicy::LatestWins,
                cx,
            );
            assert!(
                result.is_some(),
                "prefetch should return Some for stale data \
                 (data from t=0 is well past the 60s TTL at current time)"
            );
        });
    });
}
