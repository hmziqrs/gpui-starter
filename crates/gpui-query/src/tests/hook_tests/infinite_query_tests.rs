//! Tests for `use_infinite_query`, `fetch_next_page_infinite`, and
//! `fetch_previous_page_infinite`.

use std::sync::{Arc, Mutex};

use gpui::{AppContext as _, Entity, TestAppContext};

use crate::core::{CachePolicy, InfiniteQueryResource, QueryError, QueryKey, QueryStatus, RetryPolicy};
use crate::hook::*;
use crate::tests::test_support::*;

// ── use_infinite_query ─────────────────────────────────────────────────────

#[gpui::test]
fn test_use_infinite_query_creates_entity(cx: &mut TestAppContext) {
    setup_query_client(cx);

    #[allow(dead_code)]
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

// ── use_infinite_query: fetch_next_page while already fetching ──────────────

#[gpui::test]
fn test_fetch_next_page_while_fetching(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<InfiniteQueryResource<Vec<i32>, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_infinite_query(
            InfiniteQueryOptions::new("next-while-fetching")
                .cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_last_page| async move { Ok::<_, QueryError>((vec![1], true)) },
            cx,
        );
        H { entity }
    });

    // Wait for first page to load.
    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).pages().len(), 1);
    });

    // Start a fetch_next_page. This should trigger a loading state.
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
        assert_eq!(resource.status(), QueryStatus::Success);
        let pages = resource.pages();
        assert_eq!(pages.len(), 2, "should have first page + one next page");
        assert_eq!(pages[0], vec![1]);
        assert_eq!(pages[1], vec![2]);
        assert!(!resource.has_next_page());
    });
}

// ── use_infinite_query: fetch_previous_page ─────────────────────────────────

#[gpui::test]
fn test_fetch_previous_page_prepends_page(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<InfiniteQueryResource<Vec<i32>, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_infinite_query(
            InfiniteQueryOptions::new("prev-page").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_last_page| async move { Ok::<_, QueryError>((vec![5], false)) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.pages().len(), 1);
        assert_eq!(resource.pages()[0], vec![5]);
    });

    // Enable previous page flag so fetch_previous_page_infinite can proceed.
    let entity = cx.update(|cx| harness.read(cx).entity.clone());
    cx.update(|cx| {
        entity.update(cx, |r, _| {
            r.set_has_previous_page(true);
        });
    });

    // Fetch a previous page — it should be prepended.
    harness.update(cx, |this, cx| {
        fetch_previous_page_infinite(
            &this.entity,
            |_first_page| async move { Ok::<_, QueryError>((vec![1], true)) },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.pages().len(), 2);
        assert_eq!(resource.pages()[0], vec![1], "previous page should be at index 0");
        assert_eq!(resource.pages()[1], vec![5], "original page should shift to index 1");
    });
}

// ── use_infinite_query: max_pages enforcement through hook ──────────────────

#[gpui::test]
fn test_infinite_query_max_pages_enforcement(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<InfiniteQueryResource<Vec<i32>, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_infinite_query(
            InfiniteQueryOptions::new("max-pages-test")
                .max_pages(2)
                .cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_last_page| async move { Ok::<_, QueryError>((vec![1], true)) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    // First page loaded.
    cx.update(|cx| {
        assert_eq!(harness.read(cx).entity.read(cx).pages().len(), 1);
    });

    // Fetch page 2.
    harness.update(cx, |this, cx| {
        fetch_next_page_infinite(
            &this.entity,
            |_last_page| async move { Ok::<_, QueryError>((vec![2], true)) },
            cx,
        );
    });
    cx.run_until_parked();

    cx.update(|cx| {
        let pages = harness.read(cx).entity.read(cx).pages();
        assert_eq!(pages.len(), 2);
    });

    // Fetch page 3 — max_pages is 2, so page 1 should be evicted.
    harness.update(cx, |this, cx| {
        fetch_next_page_infinite(
            &this.entity,
            |_last_page| async move { Ok::<_, QueryError>((vec![3], false)) },
            cx,
        );
    });
    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        let pages = resource.pages();
        assert_eq!(pages.len(), 2, "should still have at most 2 pages");
        assert_eq!(pages[0], vec![2], "first page should have been evicted");
        assert_eq!(pages[1], vec![3], "newest page should be present");
    });
}

// ── fetch_next_page_infinite: direct call on existing entity ────────────────

#[gpui::test]
fn test_fetch_next_page_infinite_direct_call(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<InfiniteQueryResource<Vec<&'static str>, QueryError>>,
    }

    // Create entity via use_infinite_query, wait for first page, then call
    // fetch_next_page_infinite directly.
    let harness = cx.new(|cx| {
        let (entity, _sub) = use_infinite_query(
            InfiniteQueryOptions::new("direct-next").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_lp| async move { Ok::<_, QueryError>((vec!["p1"], true)) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    harness.update(cx, |this, cx| {
        fetch_next_page_infinite(
            &this.entity,
            |_lp| async move { Ok::<_, QueryError>((vec!["p2"], false)) },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.pages().len(), 2);
        assert_eq!(resource.pages()[0], vec!["p1"]);
        assert_eq!(resource.pages()[1], vec!["p2"]);
    });
}

// ── fetch_previous_page_infinite: direct call ──────────────────────────────

#[gpui::test]
fn test_fetch_previous_page_infinite_direct_call(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<InfiniteQueryResource<Vec<&'static str>, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_infinite_query(
            InfiniteQueryOptions::new("direct-prev").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_lp| async move { Ok::<_, QueryError>((vec!["p2"], false)) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    // Enable previous page flag so fetch_previous_page_infinite can proceed.
    let entity = cx.update(|cx| harness.read(cx).entity.clone());
    cx.update(|cx| {
        entity.update(cx, |r, _| {
            r.set_has_previous_page(true);
        });
    });

    harness.update(cx, |this, cx| {
        fetch_previous_page_infinite(
            &this.entity,
            |_fp| async move { Ok::<_, QueryError>((vec!["p0"], true)) },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.pages().len(), 2);
        assert_eq!(resource.pages()[0], vec!["p0"], "previous page should be prepended");
        assert_eq!(resource.pages()[1], vec!["p2"]);
    });
}

// ── use_infinite_query: error handling on first page ────────────────────────

#[gpui::test]
fn test_infinite_query_first_page_failure(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<InfiniteQueryResource<Vec<i32>, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_infinite_query(
            InfiniteQueryOptions::new("inf-fail")
                .cache_policy(CachePolicy::Ttl { ttl_ms: 0 })
                .retry_policy(RetryPolicy::no_retries()),
            |_last_page| async move { Err::<_, QueryError>(QueryError::response("page-fail")) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Failure);
        assert!(resource.pages().is_empty());
        let err = resource.error().expect("should have error");
        assert!(err.to_string().contains("page-fail"));
    });
}

// ── use_infinite_query: retry on failure ────────────────────────────────────

#[gpui::test]
fn test_infinite_query_retry_on_failure(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let call_count = Arc::new(Mutex::new(0u32));
    let cc = call_count.clone();

    struct H {
        entity: Entity<InfiniteQueryResource<Vec<i32>, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_infinite_query(
            InfiniteQueryOptions::new("inf-retry")
                .cache_policy(CachePolicy::Ttl { ttl_ms: 0 })
                .retry_policy(RetryPolicy::new(2).with_delay(0)),
            move |_last_page| {
                let cc = cc.clone();
                async move {
                    let mut n = cc.lock().unwrap();
                    *n += 1;
                    if *n < 3 {
                        Err::<_, QueryError>(QueryError::response("transient"))
                    } else {
                        Ok::<_, QueryError>((vec![42], false))
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
        assert_eq!(
            resource.status(),
            QueryStatus::Success,
            "should succeed after retries"
        );
        assert_eq!(resource.pages().len(), 1);
        assert_eq!(resource.pages()[0], vec![42]);
    });
    assert_eq!(
        *call_count.lock().unwrap(),
        3,
        "should have retried until success"
    );
}

// ── use_infinite_query: multiple pages appended sequentially ────────────────

#[gpui::test]
fn test_infinite_query_sequential_pages(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<InfiniteQueryResource<Vec<i32>, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_infinite_query(
            InfiniteQueryOptions::new("seq-pages").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_lp| async move { Ok::<_, QueryError>((vec![1], true)) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    // Fetch 3 more pages sequentially.
    for page_num in 2..=4 {
        let harness_ref = &harness;
        harness_ref.update(cx, |this, cx| {
            fetch_next_page_infinite(
                &this.entity,
                move |_lp| async move { Ok::<_, QueryError>((vec![page_num], page_num < 4)) },
                cx,
            );
        });
        cx.run_until_parked();
    }

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.pages().len(), 4);
        assert_eq!(resource.pages()[0], vec![1]);
        assert_eq!(resource.pages()[1], vec![2]);
        assert_eq!(resource.pages()[2], vec![3]);
        assert_eq!(resource.pages()[3], vec![4]);
        assert!(!resource.has_next_page());
    });
}
