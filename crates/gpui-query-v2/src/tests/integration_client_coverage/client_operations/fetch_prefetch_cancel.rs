//! Fetch, prefetch, and cancel query tests (tests 31–38).

use gpui::{AppContext as _, BorrowAppContext as _, TestAppContext};

use crate::client::QueryClient;
use crate::core::*;
use crate::tests::test_support::*;

// -- 31. prepare_fetch_query always starts (uses Force mode) -----------------

#[gpui::test]
fn test_prepare_fetch_query_uses_force_mode_always_starts(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(QueryClient::with_policies(
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        ));
    });
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // First fetch succeeds
            let prepared = client
                .prepare_fetch_query::<String, QueryError>("cached_key", cx)
                .expect("first fetch should start");
            prepared.complete_success("data".to_string(), cx);

            // prepare_fetch_query uses QueryFetchMode::Force, so it always
            // starts a new request even when the cache is fresh. This matches
            // TanStack Query's fetchQuery behavior.
            let second = client.prepare_fetch_query::<String, QueryError>("cached_key", cx);
            assert!(
                second.is_some(),
                "prepare_fetch_query uses Force mode, always starts"
            );

            // Data should still be accessible from the first fetch
            let data =
                client.get_query_data::<String, QueryError>(&QueryKey::from("cached_key"), cx);
            assert_eq!(data, Some("data".to_string()));
        });
    });
}

// -- 32. prepare_fetch_query refetch after TTL --------------------------------
//
// NOTE: This test verifies that prepare_fetch_query returns Some both on the
// initial call and on a subsequent call with Force mode. Full TTL expiry
// behavior (data becoming stale and triggering automatic refetch) is tested
// at the resource level in core_cache.rs, where timestamps can be controlled
// deterministically via apply_success(data, now_ms).

#[gpui::test]
fn test_prepare_fetch_query_refetch_after_ttl(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(QueryClient::with_policies(
            CachePolicy::Ttl { ttl_ms: 500 },
            RequestPolicy::LatestWins,
        ));
    });
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // First fetch
            let prepared = client
                .prepare_fetch_query::<String, QueryError>("ttl_key", cx)
                .expect("first fetch should start");
            prepared.complete_success("old".to_string(), cx);

            // Data should be present after first fetch
            let data = client.get_query_data::<String, QueryError>(&QueryKey::from("ttl_key"), cx);
            assert_eq!(data, Some("old".to_string()));

            // prepare_fetch_query always returns Some (Force mode), even when
            // cache is fresh. This is the core guarantee: it always initiates
            // a fetch, unlike prepare_prefetch_query which respects freshness.
            let second = client.prepare_fetch_query::<String, QueryError>("ttl_key", cx);
            assert!(
                second.is_some(),
                "prepare_fetch_query should always return Some (Force mode), \
                 even when data was just set"
            );
        });
    });
}

// -- 33. prepare_prefetch_query returns None for fresh data ------------------
//
// Finding 4/7 fix: Asserts the actual return value of prepare_prefetch_query.
// Uses the current wall-clock time via current_time_ms() to set a timestamp
// that is guaranteed fresh (age ~0ms, well within the 60s TTL).
//
// WALL-CLOCK DEPENDENCY: This test relies on current_time_ms() for both the
// data timestamp and the freshness check inside prepare_prefetch_query. The
// 60-second TTL provides a large margin against clock skew in CI, but if this
// test ever flakes, the root cause will be a system clock discontinuity (e.g.,
// NTP step). An alternative would be to mock current_time_ms, but that would
// require a crate-level time abstraction that is not currently available.

#[gpui::test]
fn test_prepare_prefetch_query_returns_none_for_fresh(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(QueryClient::with_policies(
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        ));
    });
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("prefresh_fresh");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);
            // Use the current wall-clock time so the data is fresh when
            // prepare_prefetch_query checks is_cache_fresh(current_time_ms()).
            let now = crate::client::current_time_ms();
            entity.update(cx, |r, _| r.apply_success("fresh_data".to_string(), now));

            // prepare_prefetch_query uses Normal mode. Since the data was set
            // at ~now, age is ~0ms, which is within the 60s TTL, so the cache
            // is fresh and prefetch should return None (no fetch needed).
            let result = client.prepare_prefetch_query::<String, QueryError>(
                key.clone(),
                CachePolicy::Ttl { ttl_ms: 60_000 },
                RequestPolicy::LatestWins,
                cx,
            );
            assert!(
                result.is_none(),
                "prefetch should return None for fresh data \
                 (age ~0ms, well within 60s TTL)"
            );
        });
    });
}

// -- 34. PreparedFetch complete_failure stores error -------------------------

#[gpui::test]
fn test_prepared_fetch_complete_failure_stores_error(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("pf_fail");
            let prepared = client
                .prepare_fetch_query::<String, QueryError>(key.clone(), cx)
                .expect("should start");

            let error = QueryError::response("server unavailable");
            prepared.complete_failure(error.clone(), cx);

            let entity = client.query::<String, QueryError>(&key).unwrap();
            assert_eq!(entity.read(cx).status(), QueryStatus::Failure);
            assert!(entity.read(cx).data().is_none());
            assert!(entity.read(cx).error().is_some());
        });
    });
}

// -- 35. PreparedFetch signal starts uncancelled -----------------------------

#[gpui::test]
fn test_prepared_fetch_signal_properties(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let prepared = client
                .prepare_fetch_query::<String, QueryError>("signal_test", cx)
                .expect("should start");

            assert!(
                !prepared.signal.is_cancelled(),
                "signal should start uncancelled"
            );
            assert!(
                prepared.request_id.value() > 0,
                "request_id should have a positive value"
            );

            // Complete to clean up
            prepared.complete_success("data".to_string(), cx);
        });
    });
}

// -- 36. cancel_queries cancels resources across multiple type buckets -------

#[gpui::test]
fn test_cancel_queries_across_type_buckets(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create and start requests for two different types with the same string key
            let key_s = QueryKey::from("target");
            let entity_s = client.resource::<String, QueryError>(key_s.clone(), cx);
            let rid_s = client
                .next_request_id_for_key::<String, QueryError>(&key_s)
                .expect("rid");
            entity_s.update(cx, |r, _| {
                r.begin_request_with_id(Some(rid_s), 1_000, QueryFetchMode::Normal);
            });

            let key_u = QueryKey::from("target");
            let entity_u = client.resource::<u32, QueryError>(key_u.clone(), cx);
            let rid_u = client
                .next_request_id_for_key::<u32, QueryError>(&key_u)
                .expect("rid");
            entity_u.update(cx, |r, _| {
                r.begin_request_with_id(Some(rid_u), 1_000, QueryFetchMode::Normal);
            });

            let sig_s = entity_s.read(cx).signal().unwrap().clone();
            let sig_u = entity_u.read(cx).signal().unwrap().clone();

            // Cancel all queries with key "target" (Exact filter)
            client.cancel_queries(&QueryKeyFilter::Exact(&key_s), cx);

            assert!(sig_s.is_cancelled(), "String query should be cancelled");
            assert!(sig_u.is_cancelled(), "u32 query should be cancelled");
        });
    });
}

// -- 37. cancel_queries with All filter cancels everything -------------------

#[gpui::test]
fn test_cancel_queries_all_filter(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Start two loading queries
            let key1 = QueryKey::from("a1");
            let key2 = QueryKey::from("a2");
            let e1 = client.resource::<String, QueryError>(key1.clone(), cx);
            let e2 = client.resource::<String, QueryError>(key2.clone(), cx);

            let rid1 = client
                .next_request_id_for_key::<String, QueryError>(&key1)
                .unwrap();
            let rid2 = client
                .next_request_id_for_key::<String, QueryError>(&key2)
                .unwrap();

            e1.update(cx, |r, _| {
                r.begin_request_with_id(Some(rid1), 1_000, QueryFetchMode::Normal);
            });
            e2.update(cx, |r, _| {
                r.begin_request_with_id(Some(rid2), 1_000, QueryFetchMode::Normal);
            });

            client.cancel_queries(&QueryKeyFilter::All, cx);

            let sig1 = e1.read(cx).signal().unwrap().clone();
            let sig2 = e2.read(cx).signal().unwrap().clone();
            assert!(sig1.is_cancelled());
            assert!(sig2.is_cancelled());
        });
    });
}

// -- 38. cancel_queries does not affect idle infinite queries ----------------

#[gpui::test]
fn test_cancel_queries_skips_idle_infinite_queries(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("inf_idle");
            let _entity = client.infinite_resource::<String, QueryError>(key.clone(), cx);

            // Should not panic or affect the idle infinite query
            client.cancel_queries(&QueryKeyFilter::Exact(&key), cx);

            let retrieved = client.infinite_query::<String, QueryError>(&key);
            assert!(
                retrieved.is_some(),
                "idle infinite query should still exist"
            );
        });
    });
}
