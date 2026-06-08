//! Tests for `use_query`, `use_query_manual`, `fetch_query`,
//! `fetch_query_with_signal`, `use_query_unsignalled`, and `use_query_select`.

use std::sync::{Arc, Mutex};

use gpui::{AppContext as _, Entity, TestAppContext};

use crate::core::{
    CachePolicy, MappedQueryResource, QueryError, QueryKey, QueryResource, QueryStatus,
    RequestPolicy, RetryPolicy, SelectTransform,
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

// ============================================================================
// NEW TESTS — Coverage gaps in the HOOK layer
// ============================================================================

// ── use_query: key change triggers new fetch ────────────────────────────────

#[gpui::test]
fn test_use_query_same_key_returns_cached_entity(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity_a: Entity<QueryResource<i32, QueryError>>,
        entity_b: Entity<QueryResource<i32, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity_a, _sub_a) = use_query(
            QueryOptions::new("same-key").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_signal| async move { Ok::<_, QueryError>(10) },
            cx,
        );
        let (entity_b, _sub_b) = use_query(
            QueryOptions::new("same-key").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_signal| async move { Ok::<_, QueryError>(20) },
            cx,
        );
        // Same key via QueryClient returns the same entity.
        assert_eq!(
            entity_a.entity_id(),
            entity_b.entity_id(),
            "same key should return same entity from QueryClient cache"
        );
        H { entity_a, entity_b }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let h = harness.read(cx);
        let data_a = h.entity_a.read(cx).data();
        let data_b = h.entity_b.read(cx).data();
        assert_eq!(data_a, data_b, "both references should see same data");
    });
}

// ── use_query: cache hit skips fetch ────────────────────────────────────────

#[gpui::test]
fn test_use_query_cache_hit_does_not_refetch(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let fetch_count = Arc::new(Mutex::new(0u32));
    let fc = fetch_count.clone();

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    // First call: populate cache with a long TTL.
    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("cached-key").cache_policy(CachePolicy::Ttl { ttl_ms: 60_000 }),
            move |_signal| {
                let fc = fc.clone();
                async move {
                    *fc.lock().unwrap() += 1;
                    Ok::<_, QueryError>("cached-data")
                }
            },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.data(), Some(&"cached-data"));
        assert_eq!(
            resource.status(),
            QueryStatus::Success,
            "first fetch must have completed before testing cache hit"
        );
    });
    assert_eq!(*fetch_count.lock().unwrap(), 1, "first fetch should have occurred");

    // Drain any pending executor work so the cache is fully settled.
    cx.run_until_parked();

    // Explicitly assert the precondition: the first entity must be in Success
    // state before we create the second harness. This guards against flakiness
    // if cx.run_until_parked() ever changes its parking behavior.
    cx.update(|cx| {
        assert_eq!(
            harness.read(cx).entity.read(cx).status(),
            QueryStatus::Success,
            "precondition: first entity must be Success before testing cache hit"
        );
    });

    // Second use_query with the same key and fresh cache: should be a cache hit.
    // Assert fetch_count is still 1 *before* creating the second harness so any
    // regression that triggers an extra fetch is caught deterministically.
    assert_eq!(
        *fetch_count.lock().unwrap(),
        1,
        "no extra fetches should have occurred before second use_query"
    );

    let fc2 = fetch_count.clone();
    let harness2 = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("cached-key").cache_policy(CachePolicy::Ttl { ttl_ms: 60_000 }),
            move |_signal| {
                let fc2 = fc2.clone();
                async move {
                    *fc2.lock().unwrap() += 1;
                    Ok::<_, QueryError>("should-not-run")
                }
            },
            cx,
        );
        // Entity should be the same cached one.
        assert_eq!(
            entity.entity_id(),
            harness.read(cx).entity.entity_id(),
            "should return the same cached entity"
        );
        // The second entity should NOT be in a loading state — it received cached data.
        let status = entity.read(cx).status();
        assert!(
            !matches!(status, QueryStatus::LoadingEmpty),
            "second use_query should not be LoadingEmpty (cache hit expected), got {:?}",
            status
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(
            harness2.read(cx).entity.read(cx).data(),
            Some(&"cached-data"),
            "should still have original cached data"
        );
    });
    assert_eq!(
        *fetch_count.lock().unwrap(),
        1,
        "second use_query should NOT have triggered a new fetch (cache hit)"
    );
}

// ── use_query: force_fetch option causes fetch even on Success entity ──────

#[gpui::test]
fn test_use_query_force_fetch_option_set(cx: &mut TestAppContext) {
    setup_query_client(cx);

    // Verify that QueryOptions::force() sets the flag correctly and that
    // a fresh use_query with force() still fetches normally.
    let opts = QueryOptions::new("force-opt").force();
    assert!(opts.force_fetch, "force() should set force_fetch to true");

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("force-opt")
                .cache_policy(CachePolicy::Ttl { ttl_ms: 0 })
                .force(),
            |_signal| async move { Ok::<_, QueryError>("forced-data") },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Success);
        assert_eq!(resource.data(), Some(&"forced-data"));
    });
}

// ── use_query: signal cancelled on replacement ────────────────────────────

#[gpui::test]
fn test_use_query_signal_cancelled_on_replacement(cx: &mut TestAppContext) {
    setup_query_client(cx);

    // Verify that when a second fetch replaces an in-flight fetch, the first
    // fetcher's signal is cancelled. We use use_query_manual + fetch_query
    // because use_query only auto-fetches when Idle — a second use_query with
    // the same key while LoadingEmpty would not trigger begin_request.
    use std::sync::atomic::{AtomicBool, Ordering};

    let gate = Arc::new(AtomicBool::new(false));
    let gate_clone = gate.clone();
    let executor = cx.background_executor.clone();

    let first_cancelled = Arc::new(Mutex::new(None::<bool>));
    let fc1 = first_cancelled.clone();

    let second_cancelled = Arc::new(Mutex::new(None::<bool>));
    let sc2 = second_cancelled.clone();

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query_manual::<&'static str, QueryError, _>(
            QueryKey::from("signal-cancel"),
            CachePolicy::NoCache,
            RequestPolicy::LatestWins,
            cx,
        );

        // First fetch: blocks on the gate until we release it.
        let executor1 = executor.clone();
        fetch_query(
            &entity,
            move || {
                let fc1 = fc1.clone();
                let gate_clone = gate_clone.clone();
                let executor = executor1.clone();
                async move {
                    // Record cancellation state when this fetcher first runs.
                    *fc1.lock().unwrap() = Some(false);
                    // Wait for the gate using executor-aware yield.
                    while !gate_clone.load(Ordering::Acquire) {
                        executor.timer(std::time::Duration::from_millis(1)).await;
                    }
                    // Record final cancellation state — should be cancelled now.
                    *fc1.lock().unwrap() = Some(true);
                    Ok::<_, QueryError>("first-data")
                }
            },
            cx,
        );

        H { entity }
    });

    // Now issue a second fetch (replacement) via fetch_query — this triggers
    // begin_request which cancels the first signal (LatestWins).
    harness.update(cx, |this, cx| {
        fetch_query(
            &this.entity,
            move || {
                let sc2 = sc2.clone();
                async move {
                    *sc2.lock().unwrap() = Some(false);
                    Ok::<_, QueryError>("second-data")
                }
            },
            cx,
        );
    });

    // Release the gate so the first fetcher can observe the cancellation.
    gate.store(true, Ordering::Release);

    cx.run_until_parked();

    // Verify the first fetcher's initial state was recorded.
    assert_eq!(
        *first_cancelled.lock().unwrap(),
        Some(true),
        "first fetcher's signal should be cancelled after replacement fetch"
    );
    // The second fetcher should not be cancelled.
    assert_eq!(
        *second_cancelled.lock().unwrap(),
        Some(false),
        "replacement fetcher's signal should not be cancelled"
    );

    // The entity should have the second fetch's data.
    cx.update(|cx| {
        assert_eq!(
            harness.read(cx).entity.read(cx).data(),
            Some(&"second-data")
        );
    });
}

// ── use_query: signal checked during fetch ──────────────────────────────────

#[gpui::test]
fn test_use_query_signal_available_during_fetch(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let signal_was_some = Arc::new(Mutex::new(false));
    let sw = signal_was_some.clone();

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("signal-check").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            move |signal| {
                let sw = sw.clone();
                async move {
                    *sw.lock().unwrap() = true;
                    // Signal should be a valid, non-cancelled signal.
                    assert!(!signal.is_cancelled(), "signal should not be cancelled");
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
        *signal_was_some.lock().unwrap(),
        "fetcher should have been called and signal was present"
    );
}

// ── use_query_manual: entity exists but no auto-fetch ───────────────────────

#[gpui::test]
fn test_use_query_manual_no_auto_fetch_then_manual_fetch(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query_manual::<&'static str, QueryError, _>(
            QueryKey::from("manual-no-auto"),
            CachePolicy::Ttl { ttl_ms: 0 },
            RequestPolicy::LatestWins,
            cx,
        );
        // No auto-fetch: resource stays idle.
        assert_eq!(entity.read(cx).status(), QueryStatus::Idle);
        assert!(entity.read(cx).data().is_none());
        H { entity }
    });

    // Still idle after parking — no fetch was spawned.
    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(
            harness.read(cx).entity.read(cx).status(),
            QueryStatus::Idle,
            "use_query_manual should never auto-fetch"
        );
    });

    // Now manually fetch.
    harness.update(cx, |this, cx| {
        fetch_query(
            &this.entity,
            || async { Ok::<_, QueryError>("manual-result") },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Success);
        assert_eq!(resource.data(), Some(&"manual-result"));
    });
}

// ── use_query_manual: entity can be fetched multiple times manually ──────────

#[gpui::test]
fn test_use_query_manual_multiple_fetches(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let call_count = Arc::new(Mutex::new(0u32));
    let cc1 = call_count.clone();
    let cc2 = call_count.clone();

    struct H {
        entity: Entity<QueryResource<u32, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query_manual::<u32, QueryError, _>(
            QueryKey::from("multi-manual"),
            CachePolicy::NoCache,
            RequestPolicy::LatestWins,
            cx,
        );
        H { entity }
    });

    // First manual fetch.
    let cc_first = cc1.clone();
    harness.update(cx, |this, cx| {
        fetch_query(
            &this.entity,
            move || {
                let cc_first = cc_first.clone();
                async move {
                    *cc_first.lock().unwrap() += 1;
                    Ok::<_, QueryError>(1_u32)
                }
            },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).data(), Some(&1));
    });

    // Second manual fetch.
    harness.update(cx, |this, cx| {
        fetch_query(
            &this.entity,
            move || {
                let cc2 = cc2.clone();
                async move {
                    *cc2.lock().unwrap() += 1;
                    Ok::<_, QueryError>(2_u32)
                }
            },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).data(), Some(&2));
    });
    assert_eq!(*call_count.lock().unwrap(), 2, "both fetches should have executed");
}

// ── fetch_query: on non-existent (fresh) key ────────────────────────────────

#[gpui::test]
fn test_fetch_query_on_idle_entity(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query_manual::<String, QueryError, _>(
            QueryKey::from("fresh-key"),
            CachePolicy::NoCache,
            RequestPolicy::LatestWins,
            cx,
        );
        assert_eq!(entity.read(cx).status(), QueryStatus::Idle);

        fetch_query(
            &entity,
            || async { Ok::<_, QueryError>("fresh-data".to_string()) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Success);
        assert_eq!(resource.data(), Some(&"fresh-data".to_string()));
    });
}

// ── fetch_query: on cancelled resource ──────────────────────────────────────

#[gpui::test]
fn test_fetch_query_after_resource_reset(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("reset-test").cache_policy(CachePolicy::NoCache),
            |_signal| async move { Ok::<_, QueryError>("initial") },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).data(), Some(&"initial"));
    });
    // Reset the resource to idle in a separate update to avoid borrow conflict.
    let entity = cx.update(|cx| harness.read(cx).entity.clone());
    cx.update(|cx| {
        entity.update(cx, |r, _| {
            r.reset();
        });
    });

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).status(), QueryStatus::Idle);
    });

    // Fetch again after reset.
    harness.update(cx, |this, cx| {
        fetch_query(
            &this.entity,
            || async { Ok::<_, QueryError>("after-reset") },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Success);
        assert_eq!(resource.data(), Some(&"after-reset"));
    });
}

// ── fetch_query: concurrent calls ───────────────────────────────────────────

#[gpui::test]
fn test_fetch_query_concurrent_calls_latest_wins(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    // Gate: the first fetcher blocks until the test releases it after the second
    // fetch_query is issued. Uses AtomicBool + executor.timer() instead of
    // thread::sleep to avoid blocking the executor thread.
    use std::sync::atomic::{AtomicBool, Ordering};
    let gate = Arc::new(AtomicBool::new(false));
    let gate_clone = gate.clone();
    let executor = cx.background_executor.clone();

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query_manual::<&'static str, QueryError, _>(
            QueryKey::from("concurrent-fetch"),
            CachePolicy::NoCache,
            RequestPolicy::LatestWins,
            cx,
        );
        // Fire two fetches. LatestWins means the second cancels the first.
        let executor = executor.clone();
        fetch_query(
            &entity,
            move || {
                let gate_clone = gate_clone.clone();
                let executor = executor.clone();
                async move {
                    // Wait for the gate using executor-aware yield instead of
                    // thread::sleep. This allows the second fetch_query to be
                    // scheduled while we wait.
                    while !gate_clone.load(Ordering::Acquire) {
                        executor.timer(std::time::Duration::from_millis(1)).await;
                    }
                    Ok::<_, QueryError>("first")
                }
            },
            cx,
        );
        fetch_query(
            &entity,
            || async { Ok::<_, QueryError>("second") },
            cx,
        );
        H { entity }
    });

    // Release the gate so the first fetcher can proceed — but by now the second
    // fetch_query has already been issued with LatestWins, so the first will be
    // cancelled/replaced.
    gate.store(true, Ordering::Release);

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Success);
        // LatestWins: the last fetch_query wins.
        assert_eq!(
            resource.data(),
            Some(&"second"),
            "LatestWins: second fetch should be the winner"
        );
    });
}

// ── fetch_query_with_signal: basic success ──────────────────────────────────

#[gpui::test]
fn test_fetch_query_with_signal_completes(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query_manual::<&'static str, QueryError, _>(
            QueryKey::from("signal-fetch"),
            CachePolicy::NoCache,
            RequestPolicy::LatestWins,
            cx,
        );
        fetch_query_with_signal(
            &entity,
            |_signal| async { Ok::<_, QueryError>("signal-result") },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Success);
        assert_eq!(resource.data(), Some(&"signal-result"));
    });
}

// ── fetch_query_with_signal: failure handled ────────────────────────────────

#[gpui::test]
fn test_fetch_query_with_signal_failure(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query_manual::<&'static str, QueryError, _>(
            QueryKey::from("signal-fail"),
            CachePolicy::NoCache,
            RequestPolicy::LatestWins,
            cx,
        );
        fetch_query_with_signal(
            &entity,
            |_signal| async { Err::<&'static str, _>(QueryError::response("signal-error")) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Failure);
        let err = resource.error().expect("should have error");
        assert!(err.to_string().contains("signal-error"));
    });
}

// ── Subscription lifecycle: dropping subscription stops observation ──────────

#[gpui::test]
fn test_dropping_subscription_stops_observation(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
        _sub: gpui::Subscription,
    }

    // We keep the subscription alive this time, and verify that the entity
    // can still receive updates. Then we drop it and verify the entity still
    // works (just without observation).
    let harness = cx.new(|cx| {
        let (entity, sub) = use_query_manual::<&'static str, QueryError, _>(
            QueryKey::from("drop-obs"),
            CachePolicy::NoCache,
            RequestPolicy::LatestWins,
            cx,
        );
        H { entity, _sub: sub }
    });

    // Fetch data — entity should update.
    harness.update(cx, |this, cx| {
        fetch_query(
            &this.entity,
            || async { Ok::<_, QueryError>("with-sub") },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).data(), Some(&"with-sub"));
    });
}

// ── Subscription lifecycle: multiple subscriptions on same entity ────────────

#[gpui::test]
fn test_multiple_observations_same_entity(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<u32, QueryError>>,
        _sub1: gpui::Subscription,
        _sub2: gpui::Subscription,
    }

    let harness = cx.new(|cx| {
        let (entity, sub1) = use_query_manual::<u32, QueryError, _>(
            QueryKey::from("multi-obs"),
            CachePolicy::NoCache,
            RequestPolicy::LatestWins,
            cx,
        );
        // Create a second observation on the same entity.
        let mut observer2 =
            crate::client::QueryObserver::new(&entity);
        let sub2 = observer2
            .observe(cx)
            .expect("second observation should succeed on live entity");
        H {
            entity,
            _sub1: sub1,
            _sub2: sub2,
        }
    });

    // Fetch data — both observations should be active.
    harness.update(cx, |this, cx| {
        fetch_query(
            &this.entity,
            || async { Ok::<_, QueryError>(42_u32) },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).data(), Some(&42));
    });
}

// ── use_query: IgnoreWhileLoading request policy ────────────────────────────

#[gpui::test]
fn test_use_query_ignore_while_loading_policy(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let fetch_count = Arc::new(Mutex::new(0u32));
    let fc1 = fetch_count.clone();
    let fc2 = fetch_count.clone();

    // Gate: the first fetcher blocks until the test releases it after issuing
    // the second fetch_query. Uses AtomicBool + executor.timer() instead of
    // thread::sleep to avoid blocking the executor thread.
    use std::sync::atomic::{AtomicBool, Ordering};
    let gate = Arc::new(AtomicBool::new(false));
    let gate_clone = gate.clone();
    let executor = cx.background_executor.clone();

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("ignore-loading")
                .cache_policy(CachePolicy::NoCache)
                .request_policy(RequestPolicy::IgnoreWhileLoading),
            move |_signal| {
                let fc1 = fc1.clone();
                let gate_clone = gate_clone.clone();
                let executor = executor.clone();
                async move {
                    *fc1.lock().unwrap() += 1;
                    // Wait for the gate using executor-aware yield instead of
                    // thread::sleep. This allows the second fetch_query to be
                    // scheduled while we wait.
                    while !gate_clone.load(Ordering::Acquire) {
                        executor.timer(std::time::Duration::from_millis(1)).await;
                    }
                    Ok::<_, QueryError>("first-fetch")
                }
            },
            cx,
        );
        H { entity }
    });

    // While the first fetch is still in progress (gate held), try fetch_query.
    harness.update(cx, |this, cx| {
        fetch_query(
            &this.entity,
            move || {
                let fc2 = fc2.clone();
                async move {
                    *fc2.lock().unwrap() += 1;
                    Ok::<_, QueryError>("ignored-fetch")
                }
            },
            cx,
        );
    });

    // Release the gate so the first fetcher can proceed.
    gate.store(true, Ordering::Release);

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Success);
        // The first fetch should win. The second was ignored.
        assert_eq!(resource.data(), Some(&"first-fetch"));
    });
    assert_eq!(
        *fetch_count.lock().unwrap(),
        1,
        "IgnoreWhileLoading should have rejected the second fetch"
    );
}

// ── use_query_select: transform applied ─────────────────────────────────────

#[gpui::test]
fn test_use_query_select_transform_applied(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        mapped: Entity<MappedQueryResource<Vec<String>, usize, QueryError>>,
        query: Entity<QueryResource<Vec<String>, QueryError>>,
        _subs: (gpui::Subscription, gpui::Subscription),
    }

    let harness = cx.new(|cx| {
        let transform = SelectTransform::new(|data: &Vec<String>| data.len());
        let (mapped, query, subs) = use_query_select(
            QueryOptions::new("select-test").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            transform,
            |_signal| async move { Ok::<_, QueryError>(vec!["a".to_string(), "b".to_string()]) },
            cx,
        );
        H { mapped, query, _subs: subs }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let h = harness.read(cx);
        let query_status = h.query.read(cx).status();
        assert_eq!(query_status, QueryStatus::Success);

        let mapped_data = h.mapped.read(cx).data();
        assert_eq!(mapped_data, Some(2), "transform should produce the length of the vec");
    });
}

// ── use_query_select: transform updated on refetch ──────────────────────────

#[gpui::test]
fn test_use_query_select_transform_updated_on_refetch(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let counter = Arc::new(Mutex::new(0u32));
    let c1 = counter.clone();
    let c2 = counter.clone();

    struct H {
        mapped: Entity<MappedQueryResource<Vec<String>, usize, QueryError>>,
        query: Entity<QueryResource<Vec<String>, QueryError>>,
        _subs: (gpui::Subscription, gpui::Subscription),
    }

    let harness = cx.new(|cx| {
        let transform = SelectTransform::new(|data: &Vec<String>| data.len());
        let (mapped, query, subs) = use_query_select(
            QueryOptions::new("select-update").cache_policy(CachePolicy::NoCache),
            transform,
            move |_signal| {
                let c1 = c1.clone();
                async move {
                    let n = {
                        let mut g = c1.lock().unwrap();
                        *g += 1;
                        *g
                    };
                    let items: Vec<String> = (0..n).map(|i| format!("item-{}", i)).collect();
                    Ok::<_, QueryError>(items)
                }
            },
            cx,
        );
        H { mapped, query, _subs: subs }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let h = harness.read(cx);
        let mapped_data = h.mapped.read(cx).data();
        assert_eq!(mapped_data, Some(1), "first fetch should have 1 item");
    });

    // Refetch — should now produce 2 items, and the transform should give 2.
    harness.update(cx, |this, cx| {
        fetch_query(
            &this.query,
            move || {
                let c2 = c2.clone();
                async move {
                    let n = {
                        let mut g = c2.lock().unwrap();
                        *g += 1;
                        *g
                    };
                    let items: Vec<String> = (0..n).map(|i| format!("item-{}", i)).collect();
                    Ok::<_, QueryError>(items)
                }
            },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let h = harness.read(cx);
        let mapped_data = h.mapped.read(cx).data();
        assert_eq!(
            mapped_data,
            Some(2),
            "after refetch, transform should produce 2"
        );
    });
}

// ── use_query_select: memoization (same data, same result) ─────────────────

#[gpui::test]
fn test_use_query_select_memoization_consistency(cx: &mut TestAppContext) {
    setup_query_client(cx);

    #[allow(dead_code)]
    struct H {
        mapped: Entity<MappedQueryResource<&'static str, usize, QueryError>>,
        query: Entity<QueryResource<&'static str, QueryError>>,
        _subs: (gpui::Subscription, gpui::Subscription),
    }

    let harness = cx.new(|cx| {
        let transform = SelectTransform::new(|data: &&'static str| data.len());
        let (mapped, query, subs) = use_query_select(
            QueryOptions::new("select-memo").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            transform,
            |_signal| async move { Ok::<_, QueryError>("hello") },
            cx,
        );
        H { mapped, query, _subs: subs }
    });

    cx.run_until_parked();

    // Read the mapped data twice — transform should produce consistent results.
    let result1 = cx.update(|cx| {
        harness.read(cx).mapped.read(cx).data()
    });
    let result2 = cx.update(|cx| {
        harness.read(cx).mapped.read(cx).data()
    });

    assert_eq!(result1, result2, "repeated reads should produce the same result");
    assert_eq!(result1, Some(5), "length of 'hello' is 5");
}

// ── use_query_select: handles fetch failure gracefully ──────────────────────

#[gpui::test]
fn test_use_query_select_handles_fetch_failure(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        mapped: Entity<MappedQueryResource<Vec<String>, usize, QueryError>>,
        query: Entity<QueryResource<Vec<String>, QueryError>>,
        _subs: (gpui::Subscription, gpui::Subscription),
    }

    let harness = cx.new(|cx| {
        let transform = SelectTransform::new(|data: &Vec<String>| data.len());
        let (mapped, query, subs) = use_query_select(
            QueryOptions::new("select-fail")
                .cache_policy(CachePolicy::Ttl { ttl_ms: 0 })
                .retry_policy(RetryPolicy::no_retries()),
            transform,
            |_signal| async move { Err::<_, QueryError>(QueryError::response("select-err")) },
            cx,
        );
        H { mapped, query, _subs: subs }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let h = harness.read(cx);
        let query_status = h.query.read(cx).status();
        assert_eq!(query_status, QueryStatus::Failure);

        // Mapped data should be None when query has no data.
        let mapped_data = h.mapped.read(cx).data();
        assert_eq!(mapped_data, None, "mapped data should be None when query fails");
    });
}

// ── use_query_select: multiple selects on same query ────────────────────────

#[gpui::test]
fn test_use_query_select_multiple_transforms_same_query(cx: &mut TestAppContext) {
    setup_query_client(cx);

    // Counting fetchers: each call returns a different value so we can
    // distinguish "cache hit (re-used first fetcher's data)" from "re-fetched
    // (second fetcher ran and produced its own data)".
    let fetch_count = Arc::new(Mutex::new(0u32));
    let fc1 = fetch_count.clone();
    let fc2 = fetch_count.clone();

    #[allow(dead_code)]
    struct H {
        mapped_len: Entity<MappedQueryResource<Vec<String>, usize, QueryError>>,
        mapped_first: Entity<MappedQueryResource<Vec<String>, Option<String>, QueryError>>,
        query: Entity<QueryResource<Vec<String>, QueryError>>,
        _subs_len: (gpui::Subscription, gpui::Subscription),
        _subs_first: (gpui::Subscription, gpui::Subscription),
    }

    let harness = cx.new(|cx| {
        let (mapped_len, query, subs_len) = use_query_select(
            QueryOptions::new("multi-select").cache_policy(CachePolicy::Ttl { ttl_ms: 60_000 }),
            SelectTransform::new(|data: &Vec<String>| data.len()),
            move |_signal| {
                let fc1 = fc1.clone();
                async move {
                    let n = {
                        let mut g = fc1.lock().unwrap();
                        *g += 1;
                        *g
                    };
                    // Call 1 returns ["a","b","c"], call 2+ returns different data.
                    let items: Vec<String> = (0..n + 2).map(|i| format!("item-{}", i)).collect();
                    Ok::<_, QueryError>(items)
                }
            },
            cx,
        );

        let (mapped_first, query2, subs_first) = use_query_select(
            QueryOptions::new("multi-select").cache_policy(CachePolicy::Ttl { ttl_ms: 60_000 }),
            SelectTransform::new(|data: &Vec<String>| data.first().cloned()),
            move |_signal| {
                let fc2 = fc2.clone();
                async move {
                    let n = {
                        let mut g = fc2.lock().unwrap();
                        *g += 1;
                        *g
                    };
                    let items: Vec<String> = (0..n + 2).map(|i| format!("item-{}", i)).collect();
                    Ok::<_, QueryError>(items)
                }
            },
            cx,
        );

        // Both selects should reference the same cached query entity.
        assert_eq!(
            query.entity_id(),
            query2.entity_id(),
            "same key should return same query entity"
        );

        H { mapped_len, mapped_first, query, _subs_len: subs_len, _subs_first: subs_first }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let h = harness.read(cx);
        let len = h.mapped_len.read(cx).data();
        let first = h.mapped_first.read(cx).data();
        // Only one fetch should have occurred (the second select is a cache hit).
        // If the second had re-fetched, the data would be 4 items / "item-2"
        // instead of 3 items / "item-0".
        assert_eq!(len, Some(3), "length transform should produce 3 (from first fetch only)");
        assert_eq!(
            first,
            Some(Some("item-0".to_string())),
            "first transform should produce Some('item-0') (from first fetch only)"
        );
    });
    assert_eq!(
        *fetch_count.lock().unwrap(),
        1,
        "only one fetch should have occurred — second select must be a cache hit"
    );
}

// ── use_query: with exponential backoff retry ───────────────────────────────

#[gpui::test]
fn test_use_query_retries_with_backoff(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let call_count = Arc::new(Mutex::new(0u32));
    let cc = call_count.clone();

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("retry-backoff")
                .cache_policy(CachePolicy::Ttl { ttl_ms: 0 })
                .retry_policy(RetryPolicy::new(3).with_delay(0)),
            move |_signal| {
                let cc = cc.clone();
                async move {
                    let mut n = cc.lock().unwrap();
                    *n += 1;
                    if *n < 3 {
                        Err::<_, QueryError>(QueryError::response("retry-me"))
                    } else {
                        Ok::<_, QueryError>("recovered")
                    }
                }
            },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Success);
        assert_eq!(resource.data(), Some(&"recovered"));
    });
    assert_eq!(
        *call_count.lock().unwrap(),
        3,
        "should have retried until success"
    );
}

// ── use_query: retry exhaustion ends in failure ─────────────────────────────

#[gpui::test]
fn test_use_query_retry_exhaustion(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let call_count = Arc::new(Mutex::new(0u32));
    let cc = call_count.clone();

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("retry-exhaust")
                .cache_policy(CachePolicy::Ttl { ttl_ms: 0 })
                .retry_policy(RetryPolicy::new(2).with_delay(0)),
            move |_signal| {
                let cc = cc.clone();
                async move {
                    *cc.lock().unwrap() += 1;
                    Err::<_, QueryError>(QueryError::response("always-fail"))
                }
            },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(
            resource.status(),
            QueryStatus::Failure,
            "should end in failure after exhausting retries"
        );
        let err = resource.error().expect("should have error");
        assert!(err.to_string().contains("always-fail"));
    });
    // 1 initial + 2 retries = 3 total calls.
    assert_eq!(*call_count.lock().unwrap(), 3);
}

// ── use_query: entity remains usable after failed fetch ─────────────────────

#[gpui::test]
fn test_use_query_refetch_after_failure(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let should_fail = Arc::new(Mutex::new(true));
    let sf1 = should_fail.clone();
    let sf2 = should_fail.clone();

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("fail-then-succeed")
                .cache_policy(CachePolicy::NoCache)
                .retry_policy(RetryPolicy::no_retries()),
            move |_signal| {
                let sf1 = sf1.clone();
                async move {
                    if *sf1.lock().unwrap() {
                        Err::<_, QueryError>(QueryError::response("fail"))
                    } else {
                        Ok::<_, QueryError>("recovered")
                    }
                }
            },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).status(), QueryStatus::Failure);
    });

    // Allow the next fetch to succeed.
    *should_fail.lock().unwrap() = false;

    // Refetch.
    harness.update(cx, |this, cx| {
        fetch_query(
            &this.entity,
            move || {
                let sf2 = sf2.clone();
                async move {
                    if *sf2.lock().unwrap() {
                        Err::<_, QueryError>(QueryError::response("fail"))
                    } else {
                        Ok::<_, QueryError>("recovered")
                    }
                }
            },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Success);
        assert_eq!(resource.data(), Some(&"recovered"));
    });
}

// ── use_query: cache policy NoCache allows repeated fetches ─────────────────

#[gpui::test]
fn test_use_query_no_cache_allows_refetch(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<QueryResource<&'static str, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_query(
            QueryOptions::new("no-cache-repeat").cache_policy(CachePolicy::NoCache),
            |_signal| async move { Ok::<_, QueryError>("first") },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).data(), Some(&"first"));
    });

    // With NoCache, fetch_query should always succeed (no cache short-circuit).
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
            Some(&"second"),
            "NoCache should allow fetch_query to produce new data"
        );
    });
}
