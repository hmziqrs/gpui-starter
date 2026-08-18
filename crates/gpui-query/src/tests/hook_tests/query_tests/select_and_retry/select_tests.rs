//! Tests for `use_query_select`: transform applied, updated on refetch,
//! memoization, fetch failure, and multiple selects on same query.

use std::sync::{Arc, Mutex};

use gpui::{AppContext as _, Entity, TestAppContext};

use crate::core::{
    CachePolicy, MappedQueryResource, QueryError, QueryResource, QueryStatus, RetryPolicy,
    SelectTransform,
};
use crate::hook::*;
use crate::tests::test_support::*;

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
