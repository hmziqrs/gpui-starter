//! Tests for basic QueryClient operations: creation, resource CRUD,
//! type partitioning, diagnostics, and observer creation.

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
