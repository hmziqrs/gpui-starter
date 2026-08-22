//! Basic tests for `use_query`, `use_query_manual`, `fetch_query`,
//! `use_query_unsignalled`, subscriptions, key changes, and retry policy.

use std::sync::{Arc, Mutex};

use gpui::{AppContext as _, Entity, TestAppContext};

use crate::core::{
    CachePolicy, QueryError, QueryKey, QueryResource, QueryStatus, RequestPolicy, RetryPolicy,
};
use crate::hook::*;
use crate::tests::test_support::*;

// ── use_query ──────────────────────────────────────────────────────────────

#[gpui::test]
fn test_use_query_auto_fetches(cx: &mut TestAppContext) {
    setup_query_client(cx);

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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
