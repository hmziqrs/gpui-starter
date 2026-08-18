//! Tests for InfiniteQueryResource advanced scenarios.
//!
//! Covers untested paths:
//! - InfiniteQueryResource cross-direction replacement
//! - InfiniteQueryResource cache_policy and request_policy setters
//! - InfiniteQueryResource retry_policy and set_retry_policy
//! - InfiniteQueryResource started_at / last_updated_at timestamps
//! - InfiniteQueryResource cancelled_count tracking
//! - InfiniteQueryResource error accessor after failure
//! - InfiniteQueryResource key accessor
//! - InfiniteQueryResource complete_success_with_guard for prepend
//! - InfiniteQueryResource complete_failure_with_guard clears fetching flags
//! - InfiniteQueryResource is_current_request
//! - InfiniteQueryResource multiple pages loaded then failure on next
//! - InfiniteQueryResource is_loading
//! - InfiniteQueryResource active_request_id through lifecycle

use crate::core::*;
use crate::tests::test_support::*;

// ── InfiniteQueryResource: cross-direction replacement ───────────────────

#[test]
fn infinite_query_cross_direction_replaces_request() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    // Start a next-page fetch
    let id_next = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    assert!(r.is_fetching_next_page());
    assert!(!r.is_fetching_previous_page());

    // Now start a previous-page fetch (cross-direction replacement under LatestWins)
    r.set_has_previous_page(true);
    let id_prev = r.begin_fetch_previous(&mut seq, 2_000).unwrap();
    assert!(!r.is_fetching_next_page(), "next flag should be cleared");
    assert!(r.is_fetching_previous_page());
    assert_eq!(r.cancelled_count(), 1, "previous request was cancelled");

    // Completing the old next request should fail (stale)
    assert!(!r.complete_page_success(id_next, vec!["stale_next".to_string()], true, true, 3_000));
    assert_eq!(r.ignored_results(), 1);

    // Completing the previous request should succeed
    assert!(r.complete_page_success(id_prev, vec!["page0".to_string()], false, false, 3_000));
    assert_eq!(r.page_count(), 1);
    assert_eq!(r.first_page(), Some(&vec!["page0".to_string()]));
}

#[test]
fn infinite_query_cross_direction_previous_to_next() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new_bidirectional(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    r.set_has_previous_page(true);
    let id_prev = r.begin_fetch_previous(&mut seq, 1_000).unwrap();
    assert!(r.is_fetching_previous_page());

    r.set_has_next_page(true);
    let id_next = r.begin_fetch_next(&mut seq, 2_000).unwrap();
    assert!(r.is_fetching_next_page());
    assert!(!r.is_fetching_previous_page());

    assert!(!r.complete_page_success(id_prev, vec!["stale".to_string()], false, false, 3_000));
    assert!(r.complete_page_success(id_next, vec!["page1".to_string()], true, true, 3_000));
}

// ── InfiniteQueryResource: cache_policy and request_policy setters ───────

#[test]
fn infinite_query_set_cache_policy() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    assert_eq!(r.cache_policy(), CachePolicy::Ttl { ttl_ms: 60_000 });

    r.set_cache_policy(CachePolicy::NoCache);
    assert_eq!(r.cache_policy(), CachePolicy::NoCache);
}

#[test]
fn infinite_query_set_request_policy() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    assert_eq!(r.request_policy(), RequestPolicy::LatestWins);

    r.set_request_policy(RequestPolicy::IgnoreWhileLoading);
    assert_eq!(r.request_policy(), RequestPolicy::IgnoreWhileLoading);
}

// ── InfiniteQueryResource: retry_policy ───────────────────────────────────

#[test]
fn infinite_query_default_retry_policy() {
    let r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    assert_eq!(r.retry_policy(), &RetryPolicy::default());
}

#[test]
fn infinite_query_set_retry_policy() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    let policy = RetryPolicy::new(10).with_delay(500).with_exponential_backoff();
    r.set_retry_policy(policy.clone());
    assert_eq!(r.retry_policy(), &policy);
}

// ── InfiniteQueryResource: timestamps ─────────────────────────────────────

#[test]
fn infinite_query_timestamps_on_lifecycle() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    assert!(r.started_at_ms().is_none());
    assert!(r.last_updated_at_ms().is_none());

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    assert_eq!(r.started_at_ms(), Some(1_000));
    assert!(r.last_updated_at_ms().is_none());

    r.complete_page_success(id, vec!["page1".to_string()], true, true, 2_000);
    assert_eq!(r.started_at_ms(), Some(1_000), "started_at preserved after completion");
    assert_eq!(r.last_updated_at_ms(), Some(2_000));
}

// ── InfiniteQueryResource: cancelled_count tracking ───────────────────────

#[test]
fn infinite_query_cancelled_count_on_replacement() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    let _id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    assert_eq!(r.cancelled_count(), 0);

    let _id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();
    assert_eq!(r.cancelled_count(), 1);

    let _id3 = r.begin_fetch_next(&mut seq, 3_000).unwrap();
    assert_eq!(r.cancelled_count(), 2);
}

// ── InfiniteQueryResource: error accessor ─────────────────────────────────

#[test]
fn infinite_query_error_after_failure() {
    let mut r: InfiniteQueryResource<Vec<String>, QueryError> =
        InfiniteQueryResource::new(
            QueryKey::from("items"),
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        );
    let mut seq = RequestSequencer::new();

    assert!(r.error().is_none());

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_failure(id, QueryError::transport("timeout"));

    assert!(r.error().is_some());
    assert_eq!(r.error().unwrap().kind(), QueryErrorKind::Transport);
    assert_eq!(r.error().unwrap().message(), "timeout");
}

#[test]
fn infinite_query_error_cleared_on_success() {
    let mut r: InfiniteQueryResource<Vec<String>, QueryError> =
        InfiniteQueryResource::new(
            QueryKey::from("items"),
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        );
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_failure(id1, QueryError::response("err"));
    assert!(r.error().is_some());

    let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();
    assert!(r.error().is_none(), "begin_fetch clears error");

    r.complete_page_success(id2, vec!["data".to_string()], false, true, 3_000);
    assert!(r.error().is_none());
}

// ── InfiniteQueryResource: key accessor ───────────────────────────────────

#[test]
fn infinite_query_key_accessor() {
    let r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from(["users", "42", "posts"]),
        CachePolicy::NoCache,
        RequestPolicy::LatestWins,
    );
    assert_eq!(r.key(), &QueryKey::from(["users", "42", "posts"]));
}

// ── InfiniteQueryResource: complete_success_with_guard for prepend ──────

#[test]
fn infinite_query_complete_success_with_guard_prepend() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    // First, add a page via next
    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id1, vec!["page1".to_string()], true, true, 2_000);

    // Now prepend via two-phase protocol
    r.set_has_previous_page(true);
    let id2 = r.begin_fetch_previous(&mut seq, 3_000).unwrap();
    let guard = r.accept_current_request(id2).unwrap();
    r.complete_success_with_guard(&guard, vec!["page0".to_string()], false, false, 4_000);

    assert_eq!(r.page_count(), 2);
    assert_eq!(r.first_page(), Some(&vec!["page0".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["page1".to_string()]));
    assert!(!r.has_previous_page(), "has_more=false sets has_previous_page=false");
}

// ── InfiniteQueryResource: complete_failure_with_guard clears fetching flags

#[test]
fn infinite_query_complete_failure_with_guard_clears_flags() {
    let mut r: InfiniteQueryResource<Vec<String>, QueryError> =
        InfiniteQueryResource::new(
            QueryKey::from("items"),
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        );
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    assert!(r.is_fetching_next_page());

    let guard = r.accept_current_request(id).unwrap();
    r.complete_failure_with_guard(&guard, QueryError::response("fail"));

    assert!(!r.is_fetching_next_page());
    assert!(!r.is_fetching_previous_page());
    assert!(r.signal().is_none());
    assert_eq!(r.status(), QueryStatus::Failure);
}

// ── InfiniteQueryResource: is_current_request ────────────────────────────

#[test]
fn infinite_query_is_current_request() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    assert!(r.is_current_request(id1));

    let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();
    assert!(!r.is_current_request(id1));
    assert!(r.is_current_request(id2));
    assert_eq!(r.active_request_id(), Some(id2));
}

// ── InfiniteQueryResource: multiple pages loaded then failure on next ────

#[test]
fn infinite_query_multiple_pages_then_failure_does_not_clear_pages() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    // Load 3 pages
    let id1 = r.begin_fetch_next(&mut seq, 100).unwrap();
    r.complete_page_success(id1, vec!["a".to_string()], true, true, 200);
    let id2 = r.begin_fetch_next(&mut seq, 300).unwrap();
    r.complete_page_success(id2, vec!["b".to_string()], true, true, 400);
    let id3 = r.begin_fetch_next(&mut seq, 500).unwrap();
    r.complete_page_success(id3, vec!["c".to_string()], true, true, 600);

    assert_eq!(r.page_count(), 3);

    // Fail on the 4th page
    let id4 = r.begin_fetch_next(&mut seq, 700).unwrap();
    r.complete_page_failure(id4, QueryError::transport("timeout"));

    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.page_count(), 3, "failure does not clear existing pages");
    assert!(r.is_page_data_valid());
    assert_eq!(r.first_page(), Some(&vec!["a".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["c".to_string()]));
}

// ── InfiniteQueryResource: is_loading ─────────────────────────────────────

#[test]
fn infinite_query_is_loading_reflects_status() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    assert!(!r.is_loading());

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    assert!(r.is_loading());

    r.complete_page_success(id, vec!["page1".to_string()], true, true, 2_000);
    assert!(!r.is_loading());
}

// ── InfiniteQueryResource: active_request_id through lifecycle ───────────

#[test]
fn infinite_query_active_request_id_lifecycle() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    assert!(r.active_request_id().is_none());

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    assert_eq!(r.active_request_id(), Some(id));

    // Accept clears active_request_id
    let guard = r.accept_current_request(id).unwrap();
    assert!(r.active_request_id().is_none());

    r.complete_success_with_guard(&guard, vec!["page1".to_string()], false, true, 2_000);
    assert!(r.active_request_id().is_none());
}
