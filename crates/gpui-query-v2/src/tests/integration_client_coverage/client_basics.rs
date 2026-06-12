//! Client construction, resource creation, type erasure, query data,
//! and infinite query resource tests (tests 1–17).

use gpui::{BorrowAppContext as _, TestAppContext};

use crate::client::QueryClient;
use crate::core::*;
use crate::tests::test_support::*;

// -- 1. QueryClient::new() vs Default ----------------------------------------

#[gpui::test]
fn test_client_new_equals_default(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let c1 = QueryClient::new();
        let c2 = QueryClient::default();
        cx.set_global(c1);
        let d1 = cx.update_global::<QueryClient, _>(|c, cx| c.diagnostics(cx));
        cx.set_global(c2);
        let d2 = cx.update_global::<QueryClient, _>(|c, cx| c.diagnostics(cx));
        assert_eq!(d1.query_count, d2.query_count);
        assert_eq!(d1.mutation_count, d2.mutation_count);
    });
}

// -- 2. with_policies + with_gc_time builder chaining -------------------------

#[gpui::test]
fn test_builder_chaining_with_policies_and_gc(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let client = QueryClient::with_policies(
            CachePolicy::StaleWhileRevalidate {
                ttl_ms: 500,
                stale_ms: 200,
            },
            RequestPolicy::IgnoreWhileLoading,
        )
        .with_gc_time(60_000);
        cx.set_global(client);
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = client.resource::<String, QueryError>("test", cx);
            entity.read_with(cx, |r, _| {
                assert_eq!(
                    r.cache_policy(),
                    CachePolicy::StaleWhileRevalidate {
                        ttl_ms: 500,
                        stale_ms: 200
                    }
                );
                assert_eq!(r.request_policy(), RequestPolicy::IgnoreWhileLoading);
            });
        });
    });
}

// -- 3. resource_with_policies updates existing entity policies ---------------

#[gpui::test]
fn test_resource_with_policies_updates_existing_entity(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("policy_update");
            // Create with default TTL
            let e1 = client.resource::<String, QueryError>(key.clone(), cx);
            e1.read_with(cx, |r, _| {
                assert_eq!(r.cache_policy(), CachePolicy::Ttl { ttl_ms: 60_000 });
            });

            // Same key, different policies — should update in place
            let e2 = client.resource_with_policies::<String, QueryError>(
                key.clone(),
                CachePolicy::NoCache,
                RequestPolicy::IgnoreWhileLoading,
                cx,
            );
            assert_eq!(e1.entity_id(), e2.entity_id(), "same entity returned");
            e2.read_with(cx, |r, _| {
                assert_eq!(r.cache_policy(), CachePolicy::NoCache);
                assert_eq!(r.request_policy(), RequestPolicy::IgnoreWhileLoading);
            });
        });
    });
}

// -- 4. all_queries returns empty for unregistered types ----------------------

#[gpui::test]
fn test_all_queries_empty_for_unregistered_type(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create String resources
            let _s = client.resource::<String, QueryError>("s", cx);
            // Ask for u32 queries — should be empty
            let u32s = client.all_queries::<u32, QueryError>();
            assert!(u32s.is_empty(), "no u32 queries registered");
            // String queries should have 1
            let strings = client.all_queries::<String, QueryError>();
            assert_eq!(strings.len(), 1);
        });
    });
}

// -- 5. query() returns None after remove_queries -----------------------------

#[gpui::test]
fn test_query_returns_none_after_remove_queries(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("remove_me");
            let _entity = client.resource::<String, QueryError>(key.clone(), cx);
            assert!(client.query::<String, QueryError>(&key).is_some());

            client.remove_queries(&QueryKeyFilter::Exact(&key));
            assert!(
                client.query::<String, QueryError>(&key).is_none(),
                "query should return None after remove_queries"
            );
        });
    });
}

// -- 6. Multiple type erasure: 4 different (T, E) pairs in same client -------

#[gpui::test]
fn test_four_distinct_type_pairs_in_same_client(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let e1 = client.resource::<String, QueryError>("data", cx);
            let e2 = client.resource::<u32, QueryError>("data", cx);
            let e3 = client.resource::<User, QueryError>("data", cx);
            let e4 = client.resource::<Post, QueryError>("data", cx);

            // All four must be distinct entities
            let ids = [
                e1.entity_id(),
                e2.entity_id(),
                e3.entity_id(),
                e4.entity_id(),
            ];
            for i in 0..ids.len() {
                for j in (i + 1)..ids.len() {
                    assert_ne!(ids[i], ids[j], "entities at index {i} and {j} must differ");
                }
            }

            // all_queries for each type returns exactly 1
            assert_eq!(client.all_queries::<String, QueryError>().len(), 1);
            assert_eq!(client.all_queries::<u32, QueryError>().len(), 1);
            assert_eq!(client.all_queries::<User, QueryError>().len(), 1);
            assert_eq!(client.all_queries::<Post, QueryError>().len(), 1);

            let diag = client.diagnostics(cx);
            assert_eq!(diag.query_count, 4);
        });
    });
}

// -- 7. Same T different E: full lifecycle isolation --------------------------

#[gpui::test]
fn test_different_error_types_full_isolation(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("iso");
            let e1 = client.resource::<String, QueryError>(key.clone(), cx);
            let e2 = client.resource::<String, String>(key.clone(), cx);

            // Set data on e1 only
            e1.update(cx, |r, _| r.apply_success("v1".to_string(), 1_000));
            // e2 should not have data
            assert!(e2.read(cx).data().is_none());
            // e1 should have data
            assert_eq!(e1.read(cx).data().unwrap(), "v1");
        });
    });
}

// -- 8. set_query_data + get_query_data round-trip with typed data -----------

#[gpui::test]
fn test_set_and_get_query_data_with_user_type(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let user = User::new(42, "Zara");
            client.set_query_data::<User, QueryError>(QueryKey::from("user:42"), user.clone(), cx);
            let retrieved =
                client.get_query_data::<User, QueryError>(&QueryKey::from("user:42"), cx);
            assert_eq!(retrieved, Some(user));
        });
    });
}

// -- 9. set_query_data preserves previous_data for rollback ------------------

#[gpui::test]
fn test_set_query_data_multiple_times_preserves_rollback_chain(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("chain");

            // First set — no previous data
            client.set_query_data::<String, QueryError>(key.clone(), "v1".to_string(), cx);
            let e = client.query::<String, QueryError>(&key).unwrap();
            assert!(
                e.read(cx).previous_data().is_none(),
                "first set has no previous"
            );

            // Second set — previous should be v1
            client.set_query_data::<String, QueryError>(key.clone(), "v2".to_string(), cx);
            assert_eq!(e.read(cx).data().unwrap(), "v2");
            assert_eq!(e.read(cx).previous_data().unwrap(), "v1");

            // Third set — previous should be v2 (only one level of rollback)
            client.set_query_data::<String, QueryError>(key.clone(), "v3".to_string(), cx);
            assert_eq!(e.read(cx).data().unwrap(), "v3");
            assert_eq!(e.read(cx).previous_data().unwrap(), "v2");
        });
    });
}

// -- 10. get_query_data returns None for idle resource ------------------------

#[gpui::test]
fn test_get_query_data_none_for_idle_resource(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create resource but never set data
            let _entity = client.resource::<String, QueryError>("idle_data", cx);
            let data =
                client.get_query_data::<String, QueryError>(&QueryKey::from("idle_data"), cx);
            assert!(data.is_none(), "idle resource has no data");
        });
    });
}

// -- 11. rollback_to_previous returns false when no previous data ------------

#[gpui::test]
fn test_rollback_returns_false_without_previous_data(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("no_prev");
            client.set_query_data::<String, QueryError>(key.clone(), "only".to_string(), cx);
            let entity = client.query::<String, QueryError>(&key).unwrap();
            // No previous_data was set (first set_query_data)
            let rolled_back = entity.update(cx, |r, _| r.rollback_to_previous());
            assert!(
                !rolled_back,
                "rollback should return false with no previous data"
            );
        });
    });
}

// -- 12. Infinite query resource creation and retrieval -----------------------

#[gpui::test]
fn test_infinite_resource_creates_and_deduplicates(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("inf:1");
            let e1 = client.infinite_resource::<String, QueryError>(key.clone(), cx);
            let e2 = client.infinite_resource::<String, QueryError>(key.clone(), cx);
            assert_eq!(
                e1.entity_id(),
                e2.entity_id(),
                "same key returns same entity"
            );

            let e3 = client.infinite_resource::<String, QueryError>("inf:2", cx);
            assert_ne!(e1.entity_id(), e3.entity_id());
        });
    });
}

// -- 13. infinite_query() retrieval -------------------------------------------

#[gpui::test]
fn test_infinite_query_retrieves_existing(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("inf_ret");
            let created = client.infinite_resource::<String, QueryError>(key.clone(), cx);
            let retrieved = client.infinite_query::<String, QueryError>(&key);
            assert!(retrieved.is_some());
            assert_eq!(created.entity_id(), retrieved.unwrap().entity_id());

            let missing = client.infinite_query::<String, QueryError>(&QueryKey::from("nope"));
            assert!(missing.is_none());
        });
    });
}

// -- 14. all_infinite_queries returns typed results ---------------------------

#[gpui::test]
fn test_all_infinite_queries_typed(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _e1 = client.infinite_resource::<String, QueryError>("a", cx);
            let _e2 = client.infinite_resource::<String, QueryError>("b", cx);
            let _e3 = client.infinite_resource::<u32, QueryError>("c", cx);

            let strings = client.all_infinite_queries::<String, QueryError>();
            assert_eq!(strings.len(), 2);
            let u32s = client.all_infinite_queries::<u32, QueryError>();
            assert_eq!(u32s.len(), 1);
        });
    });
}

// -- 15. infinite_resource_with_policies updates policies ---------------------

#[gpui::test]
fn test_infinite_resource_with_policies(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = client.infinite_resource_with_policies::<String, QueryError>(
                "inf_pol",
                CachePolicy::NoCache,
                RequestPolicy::IgnoreWhileLoading,
                cx,
            );
            entity.read_with(cx, |r, _| {
                assert_eq!(r.cache_policy(), CachePolicy::NoCache);
                assert_eq!(r.request_policy(), RequestPolicy::IgnoreWhileLoading);
            });
        });
    });
}

// -- 16. next_request_id_for_infinite_key monotonic sequence -----------------

#[gpui::test]
fn test_next_request_id_for_infinite_key_monotonic(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("inf_seq");
            let _entity = client.infinite_resource::<String, QueryError>(key.clone(), cx);

            let id1 = client.next_request_id_for_infinite_key::<String, QueryError>(&key);
            let id2 = client.next_request_id_for_infinite_key::<String, QueryError>(&key);
            assert!(id1.is_some());
            assert!(id2.is_some());
            assert!(id1.unwrap().value() < id2.unwrap().value());
        });
    });
}

// -- 17. next_request_id_for_infinite_key returns None for missing key ------

#[gpui::test]
fn test_next_request_id_for_infinite_key_returns_none_for_missing(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, _cx| {
            let id = client
                .next_request_id_for_infinite_key::<String, QueryError>(&QueryKey::from("ghost"));
            assert!(id.is_none());
        });
    });
}
