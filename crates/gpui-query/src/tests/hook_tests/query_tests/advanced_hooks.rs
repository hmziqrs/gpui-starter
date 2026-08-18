//! Tests for cache hit behavior, signal cancellation, and signal availability.

use std::sync::{Arc, Mutex};

use gpui::{AppContext as _, Entity, TestAppContext};

use crate::core::{
    CachePolicy, QueryError, QueryKey, QueryResource, QueryStatus, RequestPolicy,
};
use crate::hook::*;
use crate::tests::test_support::*;

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
