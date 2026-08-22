//! Tests for retry with backoff, retry exhaustion, refetch after failure,
//! and cache policy behavior.

use std::sync::{Arc, Mutex};

use gpui::{AppContext as _, Entity, TestAppContext};

use crate::core::{CachePolicy, QueryError, QueryResource, QueryStatus, RetryPolicy};
use crate::hook::*;
use crate::tests::test_support::*;

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
