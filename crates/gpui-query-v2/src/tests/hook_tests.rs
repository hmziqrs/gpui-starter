//! Hook-layer integration tests for gpui-query-v2.
//!
//! Tests use `#[gpui::test]` with `TestAppContext` to exercise the full
//! hook pipeline: entity creation, observation subscription, fetch spawning,
//! completion, and lifecycle management.
//!
//! # Context pattern
//!
//! Hook functions require `&mut Context<C>` (a component-typed context), not
//! `&mut App`. We create harness entities via `cx.new(|cx| ...)` which provides
//! `Context<Harness>`. For post-creation hook calls (e.g. `fetch_query`, `mutate`),
//! we use `harness.update(cx, |_, cx| ...)`. Harness structs store entity handles
//! so they can be inspected after async work completes.

use std::sync::{Arc, Mutex};

use gpui::{AppContext as _, Entity, TestAppContext};

use crate::core::{
    CachePolicy, InfiniteQueryResource, MutationResource, MutationStatus, QueryError, QueryKey,
    QueryResource, QueryStatus, RequestPolicy, RetryPolicy,
};
use crate::hook::*;
use crate::tests::test_support::*;

// ── use_query ──────────────────────────────────────────────────────────────

#[gpui::test]
fn test_use_query_auto_fetches(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("users"),
            |_signal| async move { Ok::<_, QueryError>("data") },
            cx,
        );
        // Immediately after use_query, the resource should be loading.
        let status = entity.read(cx).status();
        assert!(
            status.is_loading(),
            "expected loading status immediately after use_query, got {:?}",
            status
        );
        H { entity }
    });

    let _ = harness;
}

#[gpui::test]
fn test_use_query_returns_entity_with_correct_key(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<i32, QueryError>>,
    }

    let key = QueryKey::from("users-list");
    let key_clone = key.clone();
    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new(key_clone),
            |_signal| async move { Ok::<_, QueryError>(42_i32) },
            cx,
        );
        let read_key = entity.read(cx).key().clone();
        assert_eq!(read_key, key, "entity key should match the options key");
        H { entity }
    });

    let _ = harness;
}

#[gpui::test]
fn test_use_query_completes_successfully(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("item").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_signal| async move { Ok::<_, QueryError>("fetched_value") },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Success);
        assert_eq!(resource.data(), Some(&"fetched_value"));
    });
}

#[gpui::test]
fn test_use_query_completes_with_failure(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<i32, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("fail-key")
                .cache_policy(CachePolicy::Ttl { ttl_ms: 0 })
                .retry_policy(RetryPolicy::no_retries()),
            |_signal| async move { Err::<i32, _>(QueryError::response("server error")) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Failure);
        assert!(resource.data().is_none());
        let err = resource.error().expect("should have an error");
        assert!(err.to_string().contains("server error"));
    });
}

// ── use_query_with_signal ──────────────────────────────────────────────────

#[gpui::test]
fn test_use_query_signal_not_cancelled_on_normal_fetch(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let signal_cancelled = Arc::new(Mutex::new(false));
    let sc = signal_cancelled.clone();

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("signal-test").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            move |signal| {
                let sc = sc.clone();
                async move {
                    *sc.lock().unwrap() = signal.is_cancelled();
                    Ok::<_, QueryError>("ok")
                }
            },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).status(), QueryStatus::Success);
    });
    assert!(
        !*signal_cancelled.lock().unwrap(),
        "signal should NOT be cancelled during a normal fetch"
    );
}

// ── use_query_manual ───────────────────────────────────────────────────────

#[gpui::test]
fn test_use_query_manual_creates_entity_without_fetch(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query_manual::<String, QueryError, _>(
            QueryKey::from("manual-key"),
            CachePolicy::Ttl { ttl_ms: 1_000 },
            RequestPolicy::LatestWins,
            cx,
        );
        let resource = entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Idle);
        assert!(resource.data().is_none());
        assert_eq!(resource.key(), &QueryKey::from("manual-key"));
        H { entity }
    });

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).status(), QueryStatus::Idle);
    });
}

// ── fetch_query ────────────────────────────────────────────────────────────

#[gpui::test]
fn test_fetch_query_triggers_refetch_on_existing_entity(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query_manual::<String, QueryError, _>(
            QueryKey::from("refetch-key"),
            CachePolicy::Ttl { ttl_ms: 0 },
            RequestPolicy::LatestWins,
            cx,
        );
        assert_eq!(entity.read(cx).status(), QueryStatus::Idle);
        fetch_query(
            &entity,
            || async { Ok::<_, QueryError>("refetched".to_string()) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Success);
        assert_eq!(resource.data(), Some(&"refetched".to_string()));
    });
}

#[gpui::test]
fn test_fetch_query_can_refetch_after_success(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("double-fetch").cache_policy(CachePolicy::NoCache),
            |_signal| async move { Ok::<_, QueryError>("first") },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).data(), Some(&"first"));
    });

    // Refetch with different data. NoCache ensures begin_request won't short-circuit.
    harness.update(cx, |this, cx| {
        fetch_query(
            &this.entity,
            || async { Ok::<_, QueryError>("second") },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(
            harness.read(cx).entity.read(cx).data(),
            Some(&"second")
        );
    });
}

// ── use_mutation ───────────────────────────────────────────────────────────

#[gpui::test]
fn test_use_mutation_creates_idle_entity(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<MutationResource<String, String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);
        let resource = entity.read(cx);
        assert_eq!(resource.status(), MutationStatus::Idle);
        assert!(resource.data().is_none());
        H { entity }
    });

    cx.update(|cx| {
        assert_eq!(
            harness.read(cx).entity.read(cx).status(),
            MutationStatus::Idle
        );
    });
}

// ── mutate ─────────────────────────────────────────────────────────────────

#[gpui::test]
fn test_mutate_triggers_execution_and_completes(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);
        mutate(
            &entity,
            "input-vars".to_string(),
            |vars| async move { Ok::<_, QueryError>(format!("result-{}", vars)) },
            cx,
        );
        assert!(entity.read(cx).is_loading(), "should be Loading immediately");
        H { mutation: entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).mutation.read(cx);
        assert!(resource.is_success());
        assert_eq!(resource.data(), Some(&"result-input-vars".to_string()));
    });
}

#[gpui::test]
fn test_mutate_failure_stores_error(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>(
            MutationOptions::default(),
            cx,
        );
        mutate(
            &entity,
            "bad-input".to_string(),
            |_vars| async { Err::<String, _>(QueryError::response("mutation failed")) },
            cx,
        );
        H { mutation: entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).mutation.read(cx);
        assert!(resource.is_failure());
        assert!(resource.data().is_none());
        let err = resource.error().expect("should have error");
        assert!(err.to_string().contains("mutation failed"));
    });
}

// ── mutate_with_callbacks ──────────────────────────────────────────────────

#[gpui::test]
fn test_mutate_with_callbacks_success(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let success_called = Arc::new(Mutex::new(false));
    let settled_called = Arc::new(Mutex::new(false));
    let success_clone = success_called.clone();
    let settled_clone = settled_called.clone();

    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let _harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);
        mutate_with_callbacks(
            &entity,
            "vars".to_string(),
            |v| async move { Ok::<_, QueryError>(format!("ok-{}", v)) },
            MutationCallbacks::new()
                .on_success(move |data| {
                    assert_eq!(data, "ok-vars");
                    *success_clone.lock().unwrap() = true;
                })
                .on_settled(move |opt_data, opt_err| {
                    assert!(opt_data.is_some());
                    assert!(opt_err.is_none());
                    *settled_clone.lock().unwrap() = true;
                }),
            cx,
        );
        H { mutation: entity }
    });

    cx.run_until_parked();

    assert!(*success_called.lock().unwrap(), "on_success should fire");
    assert!(*settled_called.lock().unwrap(), "on_settled should fire");
}

#[gpui::test]
fn test_mutate_with_callbacks_failure(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let error_called = Arc::new(Mutex::new(false));
    let settled_called = Arc::new(Mutex::new(false));
    let error_clone = error_called.clone();
    let settled_clone = settled_called.clone();

    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>(
            MutationOptions {
                retry_policy: RetryPolicy::no_retries(),
                gc_time_ms: 300_000,
            },
            cx,
        );
        mutate_with_callbacks(
            &entity,
            "fail-input".to_string(),
            |_| async { Err::<String, _>(QueryError::response("cb-error")) },
            MutationCallbacks::<String, QueryError>::new()
                .on_error(move |err: &QueryError| {
                    assert!(err.to_string().contains("cb-error"));
                    *error_clone.lock().unwrap() = true;
                })
                .on_settled(move |opt_data, opt_err| {
                    assert!(opt_data.is_none());
                    assert!(opt_err.is_some());
                    *settled_clone.lock().unwrap() = true;
                }),
            cx,
        );
        H { mutation: entity }
    });

    cx.run_until_parked();

    assert!(*error_called.lock().unwrap(), "on_error should fire");
    assert!(*settled_called.lock().unwrap(), "on_settled should fire on failure");
}

// ── use_infinite_query ─────────────────────────────────────────────────────

#[gpui::test]
fn test_use_infinite_query_creates_entity(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<InfiniteQueryResource<Vec<i32>, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_infinite_query(
            InfiniteQueryOptions::new("feed").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_last_page| async move { Ok::<_, QueryError>((vec![1, 2, 3], true)) },
            cx,
        );
        let resource = entity.read(cx);
        assert!(resource.status().is_loading());
        assert_eq!(resource.key(), &QueryKey::from("feed"));
        H { entity }
    });

    let _ = harness;
}

#[gpui::test]
fn test_use_infinite_query_fetches_first_page(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<InfiniteQueryResource<Vec<&'static str>, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_infinite_query(
            InfiniteQueryOptions::new("pages").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_last_page| async move { Ok::<_, QueryError>((vec!["a", "b"], true)) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Success);
        let pages = resource.pages();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0], vec!["a", "b"]);
        assert!(resource.has_next_page());
    });
}

#[gpui::test]
fn test_fetch_next_page_appends_page(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<InfiniteQueryResource<Vec<i32>, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_infinite_query(
            InfiniteQueryOptions::new("multi-page").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_last_page| async move { Ok::<_, QueryError>((vec![1], true)) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).pages().len(), 1);
    });

    // Fetch the next page.
    harness.update(cx, |this, cx| {
        fetch_next_page_infinite(
            &this.entity,
            |_last_page| async move { Ok::<_, QueryError>((vec![2], false)) },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.pages().len(), 2);
        assert_eq!(resource.pages()[0], vec![1]);
        assert_eq!(resource.pages()[1], vec![2]);
        assert!(!resource.has_next_page());
    });
}

// ── Subscription lifecycle ─────────────────────────────────────────────────

#[gpui::test]
fn test_subscription_drops_gracefully(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, sub) = use_query_manual::<String, QueryError, _>(
            QueryKey::from("sub-lifecycle"),
            CachePolicy::Ttl { ttl_ms: 1_000 },
            RequestPolicy::LatestWins,
            cx,
        );
        assert_eq!(entity.read(cx).status(), QueryStatus::Idle);
        // Drop the subscription inside the context. Entity should remain valid.
        drop(sub);
        assert_eq!(entity.read(cx).status(), QueryStatus::Idle);
        H { entity }
    });

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).status(), QueryStatus::Idle);
    });
}

#[gpui::test]
fn test_multiple_subscriptions_same_key(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity1: Entity<QueryResource<i32, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity1, sub1) = use_query_manual::<i32, QueryError, _>(
            QueryKey::from("multi-sub"),
            CachePolicy::Ttl { ttl_ms: 1_000 },
            RequestPolicy::LatestWins,
            cx,
        );
        let (entity2, sub2) = use_query_manual::<i32, QueryError, _>(
            QueryKey::from("multi-sub"),
            CachePolicy::Ttl { ttl_ms: 1_000 },
            RequestPolicy::LatestWins,
            cx,
        );

        // Same key = same entity from QueryClient.
        assert_eq!(entity1.entity_id(), entity2.entity_id());

        // Both subscriptions should be droppable without issues.
        drop(sub1);
        drop(sub2);

        H { entity1 }
    });

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity1.read(cx).status(), QueryStatus::Idle);
    });
}

// ── Key change triggers new fetch ──────────────────────────────────────────

#[gpui::test]
fn test_different_keys_create_distinct_entities(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity_a: Entity<QueryResource<&'static str, QueryError>>,
        entity_b: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity_a, _sub_a) = use_query(
            QueryOptions::new("key-a").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_signal| async move { Ok::<_, QueryError>("data-a") },
            cx,
        );
        let (entity_b, _sub_b) = use_query(
            QueryOptions::new("key-b").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_signal| async move { Ok::<_, QueryError>("data-b") },
            cx,
        );
        assert_ne!(entity_a.entity_id(), entity_b.entity_id());
        H { entity_a, entity_b }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let h = harness.read(cx);
        assert_eq!(h.entity_a.read(cx).data(), Some(&"data-a"));
        assert_eq!(h.entity_b.read(cx).data(), Some(&"data-b"));
    });
}

// ── Retry policy propagation ───────────────────────────────────────────────

#[gpui::test]
fn test_use_query_propagates_retry_policy_to_entity(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let custom_retry = RetryPolicy::new(5);

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("retry-test").retry_policy(custom_retry.clone()),
            |_signal| async move { Ok::<_, QueryError>("ok") },
            cx,
        );
        let policy = entity.read(cx).retry_policy().clone();
        assert_eq!(policy.max_retries, custom_retry.max_retries);
        H { entity }
    });

    let _ = harness;
}

// ── use_query_unsignalled ──────────────────────────────────────────────────

#[gpui::test]
fn test_use_query_unsignalled_auto_fetches(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<u32, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query_unsignalled(
            QueryKey::from("unsig-key"),
            CachePolicy::Ttl { ttl_ms: 0 },
            RequestPolicy::LatestWins,
            || async { Ok::<_, QueryError>(99_u32) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Success);
        assert_eq!(resource.data(), Some(&99));
    });
}

// ── Concurrent mutate guard ────────────────────────────────────────────────

#[gpui::test]
fn test_mutate_rejects_concurrent_calls(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);

        // Start the first mutation.
        mutate(
            &entity,
            "first".to_string(),
            |_vars| async move { Ok::<_, QueryError>("first-result".to_string()) },
            cx,
        );
        assert!(entity.read(cx).is_loading());

        // Attempt a second mutate while the first is still loading.
        // The second call should be rejected (no-op) per audit fix #8.
        mutate(
            &entity,
            "second".to_string(),
            |_vars| async move { Ok::<_, QueryError>("second-result".to_string()) },
            cx,
        );

        H { mutation: entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).mutation.read(cx);
        assert!(resource.is_success());
        assert_eq!(resource.variables(), Some(&"first".to_string()));
        assert_eq!(resource.data(), Some(&"first-result".to_string()));
    });
}

// ── use_query force_fetch ──────────────────────────────────────────────────

#[gpui::test]
fn test_fetch_query_refetch_after_success(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<i32, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("force-test").cache_policy(CachePolicy::NoCache),
            |_signal| async move { Ok::<_, QueryError>(1_i32) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).data(), Some(&1));
    });

    // Refetch with different data.
    harness.update(cx, |this, cx| {
        fetch_query(
            &this.entity,
            || async { Ok::<_, QueryError>(2_i32) },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).data(), Some(&2));
    });
}

// ── Mutation registered with QueryClient ────────────────────────────────────

#[gpui::test]
fn test_use_mutation_registers_with_client(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<MutationResource<String, String, QueryError>>,
    }

    let _harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);
        let mutations = use_mutation_state::<String, String, QueryError, _>(cx);
        assert_eq!(mutations.len(), 1, "one mutation should be registered");
        assert_eq!(mutations[0].entity_id(), entity.entity_id());
        H { entity }
    });
}
