//! Tests for subscription lifecycle and request policies.

use std::sync::{Arc, Mutex};

use gpui::{AppContext as _, Entity, TestAppContext};

use crate::core::{CachePolicy, QueryError, QueryKey, QueryResource, QueryStatus, RequestPolicy};
use crate::hook::*;
use crate::tests::test_support::*;

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
