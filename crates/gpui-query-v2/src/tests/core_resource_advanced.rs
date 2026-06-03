//! Tests for QueryResource and InfiniteQueryResource advanced scenarios.
//!
//! Covers untested paths:
//! - Error recovery: Failure -> Success, Failure -> Cancel, Cancel -> Success
//! - signal_mut accessor
//! - set_retry_policy and retry_policy interaction
//! - QueryResource serde roundtrip with data
//! - InfiniteQueryResource cross-direction replacement
//! - InfiniteQueryResource cache_policy and request_policy setters
//! - InfiniteQueryResource retry_policy and set_retry_policy
//! - InfiniteQueryResource started_at / last_updated_at timestamps
//! - InfiniteQueryResource cancelled_count tracking
//! - InfiniteQueryResource error accessor after failure
//! - QueryResource mark_ignored_result
//! - CachePolicy::NoCache should_clear_data_on_complete
//! - QueryBeginResult debug output variants
//! - Error recovery full cycle through Failure back to Success

use crate::core::*;
use crate::tests::test_support::*;

// ── QueryResource: Error recovery paths ────────────────────────────────────

#[test]
fn failure_to_success_recovery_cycle() {
    // Load -> Fail -> Retry -> Succeed
    let mut r: QueryResource<&str> = QueryResource::new(
        "test",
        CachePolicy::NoCache,
        RequestPolicy::LatestWins,
    );
    let mut s = test_sequencer();

    // First fetch succeeds
    let rid1 = match r.begin_request(&mut s, 100, QueryFetchMode::Normal) {
        QueryBeginResult::Started { request_id, .. } => request_id,
        _ => panic!("expected Started"),
    };
    r.complete_current_success(rid1, "v1", 200);

    // Refetch fails
    let rid2 = match r.begin_request(&mut s, 1_500, QueryFetchMode::Normal) {
        QueryBeginResult::Started { request_id, .. } => request_id,
        _ => panic!("expected Started"),
    };
    r.complete_current_failure(rid2, QueryError::transport("timeout"), 1_600);
    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.data(), Some(&"v1"), "failure retains prior data");

    // Retry succeeds (NoCache ensures we get Started, not CacheHit)
    let rid3 = match r.begin_request(&mut s, 2_000, QueryFetchMode::Normal) {
        QueryBeginResult::Started { request_id, .. } => request_id,
        _ => panic!("expected Started"),
    };
    r.complete_current_success(rid3, "v2", 2_100);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"v2"));
    assert!(r.error().is_none());
    assert_eq!(r.previous_data(), Some(&"v1"));
}

#[test]
fn cancel_then_fresh_begin_succeeds() {
    let mut r = test_resource();
    let mut s = test_sequencer();

    let _rid = match r.begin_request(&mut s, 100, QueryFetchMode::Normal) {
        QueryBeginResult::Started { request_id, .. } => request_id,
        _ => panic!("expected Started"),
    };
    r.cancel(QueryError::cancelled("abort"));
    assert_eq!(r.status(), QueryStatus::Cancelled);

    // Begin a new request after cancellation
    let rid2 = match r.begin_request(&mut s, 200, QueryFetchMode::Normal) {
        QueryBeginResult::Started { request_id, .. } => request_id,
        _ => panic!("expected Started"),
    };
    assert_eq!(r.status(), QueryStatus::LoadingEmpty);
    r.complete_current_success(rid2, "recovered", 300);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"recovered"));
}

#[test]
fn failure_with_data_then_success_updates_data() {
    let mut r: QueryResource<&str> = QueryResource::new(
        "test",
        CachePolicy::NoCache,
        RequestPolicy::LatestWins,
    );
    let mut s = test_sequencer();

    let rid = match r.begin_request(&mut s, 100, QueryFetchMode::Normal) {
        QueryBeginResult::Started { request_id, .. } => request_id,
        _ => panic!("expected Started"),
    };
    r.complete_current_failure_with_data(
        rid,
        "fallback",
        QueryError::response("partial fail"),
        200,
    );
    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.data(), Some(&"fallback"));

    // Now succeed — should update data and clear error
    let rid2 = match r.begin_request(&mut s, 300, QueryFetchMode::Normal) {
        QueryBeginResult::Started { request_id, .. } => request_id,
        _ => panic!("expected Started"),
    };
    r.complete_current_success(rid2, "fresh", 400);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"fresh"));
    assert!(r.error().is_none());
}

// ── QueryResource: signal_mut accessor ────────────────────────────────────

#[test]
fn signal_mut_returns_signal_when_active() {
    let mut r = test_resource();
    let mut s = test_sequencer();
    r.begin_request(&mut s, 100, QueryFetchMode::Normal);

    let sig = r.signal_mut();
    assert!(sig.is_some());
    assert!(!sig.unwrap().is_cancelled());
}

#[test]
fn signal_mut_returns_none_when_idle() {
    let mut r = test_resource();
    assert!(r.signal_mut().is_none());
}

// ── QueryResource: set_retry_policy ────────────────────────────────────────

#[test]
fn set_retry_policy_updates_policy() {
    let mut r = test_resource();
    assert_eq!(r.retry_policy().max_retries, 0); // default is no_retries

    let new_policy = RetryPolicy::new(5).with_delay(200).with_exponential_backoff();
    r.set_retry_policy(new_policy.clone());
    assert_eq!(r.retry_policy(), &new_policy);
    assert_eq!(r.retry_policy().max_retries, 5);
}

#[test]
fn retry_policy_preserved_across_reset() {
    let mut r = test_resource();
    r.set_retry_policy(RetryPolicy::new(10));
    r.increment_retry();
    r.reset();
    assert_eq!(r.retry_policy().max_retries, 10, "policy preserved after reset");
    assert_eq!(r.retry_count(), 0, "count cleared after reset");
}

// ── QueryResource: serde roundtrip ────────────────────────────────────────

#[test]
fn serde_roundtrip_with_data_and_error_state() {
    let mut r: QueryResource<String, QueryError> = QueryResource::new(
        "serde-test",
        CachePolicy::Ttl { ttl_ms: 5_000 },
        RequestPolicy::LatestWins,
    );
    let mut s = test_sequencer();
    let rid = match r.begin_request(&mut s, 100, QueryFetchMode::Normal) {
        QueryBeginResult::Started { request_id, .. } => request_id,
        _ => panic!("expected Started"),
    };
    r.complete_current_success(rid, "hello".to_string(), 200);

    let json = serde_json::to_string(&r).unwrap();
    let back: QueryResource<String, QueryError> = serde_json::from_str(&json).unwrap();

    assert_eq!(back.status(), QueryStatus::Success);
    assert_eq!(back.data(), Some(&"hello".to_string()));
    assert_eq!(back.key().as_str(), "serde-test");
    assert_eq!(back.cache_policy(), CachePolicy::Ttl { ttl_ms: 5_000 });
    assert!(back.signal().is_none(), "signal is #[serde(skip)]");
    assert!(back.initial_data().is_none(), "initial_data is #[serde(skip)]");
}

// ── QueryResource: mark_ignored_result ────────────────────────────────────

#[test]
fn mark_ignored_result_increments_counter() {
    let mut r = test_resource();
    assert_eq!(r.ignored_results(), 0);

    r.mark_ignored_result();
    assert_eq!(r.ignored_results(), 1);

    r.mark_ignored_result();
    r.mark_ignored_result();
    assert_eq!(r.ignored_results(), 3);
}

// ── QueryResource: set_request_policy ──────────────────────────────────────

#[test]
fn set_request_policy_changes_policy() {
    let mut r = test_resource();
    assert_eq!(r.request_policy(), RequestPolicy::LatestWins);

    r.set_request_policy(RequestPolicy::IgnoreWhileLoading);
    assert_eq!(r.request_policy(), RequestPolicy::IgnoreWhileLoading);
}

#[test]
fn set_request_policy_preserved_across_reset() {
    let mut r = test_resource();
    r.set_request_policy(RequestPolicy::IgnoreWhileLoading);
    r.reset();
    assert_eq!(r.request_policy(), RequestPolicy::IgnoreWhileLoading);
}

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

// ── QueryResource: nocache should_clear_data_on_complete ──────────────────

#[test]
fn ttl_resource_should_not_clear_data_on_complete() {
    let r = test_resource();
    assert!(!r.should_clear_data_on_complete());
}

// ── QueryResource: full recovery from cancelled state ─────────────────────

#[test]
fn cancelled_to_loading_to_success_recovery() {
    let mut r = test_resource();
    let mut s = test_sequencer();

    // Cancel a request
    let _ = r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    r.cancel(QueryError::cancelled("abort"));
    assert_eq!(r.status(), QueryStatus::Cancelled);

    // Begin a new request
    let rid2 = match r.begin_request(&mut s, 200, QueryFetchMode::Normal) {
        QueryBeginResult::Started { request_id, .. } => request_id,
        _ => panic!("expected Started"),
    };
    assert_eq!(r.status(), QueryStatus::LoadingEmpty);
    assert!(r.error().is_none(), "begin clears error");

    // Complete successfully
    r.complete_current_success(rid2, "recovered", 300);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"recovered"));
}

// ── QueryResource: begin_request_with_id respects IgnoreWhileLoading ─────

#[test]
fn begin_request_with_id_respects_ignore_while_loading() {
    let mut r: QueryResource<&str> = QueryResource::new(
        "test",
        CachePolicy::NoCache,
        RequestPolicy::IgnoreWhileLoading,
    );
    let custom_id = RequestId::scoped(10, 1);
    r.begin_request_with_id(Some(custom_id), 100, QueryFetchMode::Normal);
    assert_eq!(r.active_request_id(), Some(custom_id));

    // Second request should be ignored
    let result = r.begin_request_with_id(Some(RequestId::scoped(10, 2)), 200, QueryFetchMode::Normal);
    match result {
        QueryBeginResult::IgnoredWhileLoading { active_request_id } => {
            assert_eq!(active_request_id, custom_id);
        }
        _ => panic!("expected IgnoredWhileLoading, got {:?}", result),
    }
    assert_eq!(r.active_request_id(), Some(custom_id));
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
