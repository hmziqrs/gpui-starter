//! Comprehensive tests for the core lifecycle of QueryResource (v2).
//!
//! Covers all state transitions, cancellation, stale request rejection,
//! reset, retry counter management, signal lifecycle, and request policies.

use crate::core::*;
use crate::tests::test_support::*;

// ── Helpers ────────────────────────────────────────────────────────────

/// Create a default test resource with LatestWins policy.
fn resource() -> QueryResource<&'static str> {
    test_resource()
}

/// Create a resource with IgnoreWhileLoading policy.
fn resource_ignore() -> QueryResource<&'static str> {
    test_resource_with_policies(
        "test",
        CachePolicy::Ttl { ttl_ms: 1_000 },
        RequestPolicy::IgnoreWhileLoading,
    )
}

/// Create a fresh sequencer.
fn seq() -> RequestSequencer {
    test_sequencer()
}

/// Extract the error display string from a resource.
fn err_str(r: &QueryResource<&'static str>) -> Option<String> {
    r.error().map(|e| e.to_string())
}

/// Begin a request, returning (request_id, status).
fn begin(
    r: &mut QueryResource<&'static str>,
    seq: &mut RequestSequencer,
    now_ms: u128,
) -> (RequestId, QueryStatus) {
    match r.begin_request(seq, now_ms, QueryFetchMode::Normal) {
        QueryBeginResult::Started {
            request_id,
            status,
            ..
        } => (request_id, status),
        QueryBeginResult::StaleCacheHit {
            request_id,
            status,
            ..
        } => (request_id, status),
        other => panic!("expected Started or StaleCacheHit, got {:?}", other),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Idle -> LoadingEmpty
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn idle_to_loading_empty_transitions_correctly() {
    let mut r = resource();
    let mut s = seq();

    assert_eq!(r.status(), QueryStatus::Idle);
    assert_eq!(r.data(), None);
    assert!(r.active_request_id().is_none());

    let (rid, status) = begin(&mut r, &mut s, 100);

    assert_eq!(status, QueryStatus::LoadingEmpty);
    assert_eq!(r.status(), QueryStatus::LoadingEmpty);
    assert!(r.is_loading());
    assert!(r.is_pending());
    assert_eq!(r.active_request_id(), Some(rid));
    assert_eq!(r.started_at_ms(), Some(100));
    assert_eq!(
        r.error(),
        None,
        "error should be cleared on begin_loading"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 2. LoadingEmpty -> Success
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn loading_empty_to_success_with_data() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);

    assert!(r.complete_current_success(rid, "hello", 200));

    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"hello"));
    assert!(r.active_request_id().is_none());
    assert_eq!(r.last_updated_at_ms(), Some(200));
    assert!(r.error().is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 3. LoadingEmpty -> Failure
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn loading_empty_to_failure() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);

    assert!(r.complete_current_failure(
        rid,
        QueryError::response("server error"),
        200
    ));

    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(
        r.data(),
        None,
        "no prior data, so data should remain None"
    );
    assert_eq!(
        err_str(&r),
        Some("response error: server error".to_string())
    );
    assert!(r.active_request_id().is_none());
    assert_eq!(r.last_updated_at_ms(), Some(200));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Idle -> LoadingWithData (refetch with existing cached data)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn success_to_loading_with_data_on_refetch() {
    let mut r = resource();
    let mut s = seq();

    // Seed data via a successful fetch
    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "cached", 200));

    // Refetch (beyond TTL so cache doesn't short-circuit)
    let (rid2, status) = begin(&mut r, &mut s, 1_500);

    assert_eq!(status, QueryStatus::LoadingWithData);
    assert_eq!(r.status(), QueryStatus::LoadingWithData);
    assert_eq!(
        r.data(),
        Some(&"cached"),
        "prior data is preserved during refetch"
    );
    assert!(r.is_loading());
    assert!(!r.is_pending());
    assert_eq!(r.active_request_id(), Some(rid2));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. LoadingWithData -> Success (data updated)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn loading_with_data_to_success_updates_data() {
    let mut r = resource();
    let mut s = seq();

    // First fetch
    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "old", 200));

    // Refetch
    let (rid2, _) = begin(&mut r, &mut s, 1_500);
    assert!(r.complete_current_success(rid2, "new", 1_600));

    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"new"));
    assert_eq!(
        r.previous_data(),
        Some(&"old"),
        "previous_data holds old value"
    );
    assert_eq!(r.last_updated_at_ms(), Some(1_600));
}

// ═══════════════════════════════════════════════════════════════════════
// 6. LoadingWithData -> Failure (data retained; only cancel clears data)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn loading_with_data_to_failure_retains_data() {
    let mut r = resource();
    let mut s = seq();

    // First fetch succeeds
    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "cached", 200));

    // Refetch fails
    let (rid2, _) = begin(&mut r, &mut s, 1_500);
    assert!(r.complete_current_failure(
        rid2,
        QueryError::transport("timeout"),
        1_600
    ));

    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(
        r.data(),
        Some(&"cached"),
        "apply_failure retains existing data (only cancel() clears it)"
    );
    assert_eq!(
        err_str(&r),
        Some("transport error: timeout".to_string())
    );
    assert_eq!(
        r.last_updated_at_ms(),
        Some(1_600),
        "failure updates last_updated_at"
    );
    // Cached data is still within TTL window relative to its original timestamp,
    // but the status is Failure, not Success.
    assert!(r.is_data_stale(), "data with Failure status is stale");
}

#[test]
fn failure_with_data_retains_data() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "old", 200));

    let (rid2, _) = begin(&mut r, &mut s, 1_500);
    assert!(r.complete_current_failure_with_data(
        rid2,
        "stale-fallback",
        QueryError::response("err"),
        1_600
    ));

    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.data(), Some(&"stale-fallback"));
    assert_eq!(err_str(&r), Some("response error: err".to_string()));
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Cancellation from LoadingEmpty
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cancel_from_loading_empty() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);

    assert!(r.cancel(QueryError::cancelled("user abort")));

    assert_eq!(r.status(), QueryStatus::Cancelled);
    assert!(r.active_request_id().is_none());
    assert_eq!(r.data(), None);
    assert_eq!(err_str(&r), Some("cancelled: user abort".to_string()));
    assert_eq!(r.cancelled_count(), 1);
    // The stale rid should no longer be accepted
    assert!(r.accept_current_request(rid).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Cancellation from LoadingWithData saves previous_data
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cancel_from_loading_with_data_saves_previous_data() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "cached", 200));

    let (_rid2, _) = begin(&mut r, &mut s, 1_500);
    assert!(r.cancel(QueryError::cancelled("aborted")));

    assert_eq!(r.status(), QueryStatus::Cancelled);
    assert_eq!(r.data(), None, "cancel clears data");
    assert_eq!(
        r.previous_data(),
        Some(&"cached"),
        "cancel saves prior data to previous_data for rollback"
    );
    assert_eq!(r.last_updated_at_ms(), Some(200), "timestamp preserved");
}

#[test]
fn rollback_to_previous_restores_data_after_cancel() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "cached", 200));

    let (_rid2, _) = begin(&mut r, &mut s, 1_500);
    assert!(r.cancel(QueryError::cancelled("aborted")));

    assert!(r.rollback_to_previous());

    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"cached"));
    assert_eq!(r.previous_data(), None);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Cancel without active request is a no-op
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cancel_without_active_request_returns_false() {
    let mut r = resource();

    assert!(!r.cancel(QueryError::cancelled("nope")));
    assert_eq!(r.status(), QueryStatus::Idle);
    assert_eq!(r.cancelled_count(), 0);
}

#[test]
fn cancel_after_completion_is_noop() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "done", 200));

    // No active request anymore
    assert!(!r.cancel(QueryError::cancelled("late")));
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"done"));
    assert_eq!(r.cancelled_count(), 0);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Cancel signal lifecycle: new signal created, old signal cancelled
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn begin_request_creates_fresh_signal() {
    let mut r = resource();
    let mut s = seq();

    assert!(r.signal().is_none(), "no signal before first request");

    let _ = r.begin_request(&mut s, 100, QueryFetchMode::Normal);

    let sig = r.signal().expect("signal must exist after begin_request");
    assert!(!sig.is_cancelled(), "fresh signal must not be cancelled");
}

#[test]
fn cancel_propagates_to_signal() {
    let mut r = resource();
    let mut s = seq();

    let _ = r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    let clone = r.signal().unwrap().clone();

    assert!(r.cancel(QueryError::cancelled("aborted")));

    assert!(clone.is_cancelled(), "cloned signal must see cancellation");
    assert!(r.signal().unwrap().is_cancelled());
}

#[test]
fn new_request_cancels_old_signal() {
    let mut r = resource();
    let mut s = seq();

    let _ = r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    let old_signal = r.signal().unwrap().clone();

    // Second request replaces the first (LatestWins)
    let _ = r.begin_request(&mut s, 200, QueryFetchMode::Normal);

    assert!(
        old_signal.is_cancelled(),
        "old signal must be cancelled on replacement"
    );
    let new_signal = r.signal().unwrap();
    assert!(!new_signal.is_cancelled(), "new signal must be fresh");
    assert_ne!(
        old_signal, *new_signal,
        "signals must be distinct objects"
    );
}

#[test]
fn completion_does_not_cancel_signal() {
    let mut r = resource();
    let mut s = seq();

    let result = r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    let rid = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        _ => panic!("expected Started"),
    };

    assert!(r.complete_current_success(rid, "data", 200));

    let sig = r.signal().expect("signal persists after completion");
    assert!(
        !sig.is_cancelled(),
        "completion should not cancel signal"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Stale request rejection: old results don't overwrite new
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn accept_rejects_stale_request_id() {
    let mut r = resource();
    let mut s = seq();

    let (rid1, _) = begin(&mut r, &mut s, 100);
    let (rid2, _) = begin(&mut r, &mut s, 200);

    // rid1 is stale -- rid2 replaced it
    assert!(r.accept_current_request(rid1).is_none());
    assert_eq!(r.ignored_results(), 1);
    assert_eq!(r.active_request_id(), Some(rid2));
}

#[test]
fn stale_success_does_not_overwrite_newer_request() {
    let mut r = resource();
    let mut s = seq();

    let (rid1, _) = begin(&mut r, &mut s, 100);
    let (rid2, _) = begin(&mut r, &mut s, 200);

    // Stale completion for rid1 should be rejected
    assert!(!r.complete_current_success(rid1, "stale", 300));

    assert_eq!(r.status(), QueryStatus::LoadingEmpty);
    assert_eq!(r.data(), None);
    assert_eq!(r.active_request_id(), Some(rid2));
    assert_eq!(r.ignored_results(), 1);
}

#[test]
fn stale_failure_does_not_overwrite_newer_request() {
    let mut r = resource();
    let mut s = seq();

    let (rid1, _) = begin(&mut r, &mut s, 100);
    let (rid2, _) = begin(&mut r, &mut s, 200);

    assert!(!r.complete_current_failure(
        rid1,
        QueryError::response("old err"),
        300
    ));

    assert_eq!(r.status(), QueryStatus::LoadingEmpty);
    assert_eq!(r.active_request_id(), Some(rid2));
    assert!(r.error().is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Reset from every state
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn reset_from_idle() {
    let mut r = resource();
    r.reset();
    assert_eq!(r.status(), QueryStatus::Idle);
    assert_eq!(r.data(), None);
}

#[test]
fn reset_from_loading_empty() {
    let mut r = resource();
    let mut s = seq();
    let _ = begin(&mut r, &mut s, 100);

    r.reset();

    assert_eq!(r.status(), QueryStatus::Idle);
    assert!(r.active_request_id().is_none());
    assert_eq!(r.data(), None);
    assert!(r.signal().is_none());
}

#[test]
fn reset_from_success() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "data", 200));
    r.increment_retry();

    r.reset();

    assert_eq!(r.status(), QueryStatus::Idle);
    assert_eq!(r.data(), None);
    assert_eq!(r.error(), None);
    assert!(r.active_request_id().is_none());
    assert_eq!(r.started_at_ms(), None);
    assert_eq!(r.last_updated_at_ms(), None);
    assert_eq!(r.retry_count(), 0);
}

#[test]
fn reset_from_failure() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_failure(
        rid,
        QueryError::response("err"),
        200
    ));

    r.reset();

    assert_eq!(r.status(), QueryStatus::Idle);
    assert_eq!(r.data(), None);
    assert_eq!(r.error(), None);
}

#[test]
fn reset_from_cancelled() {
    let mut r = resource();
    let mut s = seq();

    let _ = begin(&mut r, &mut s, 100);
    assert!(r.cancel(QueryError::cancelled("abort")));

    r.reset();

    assert_eq!(r.status(), QueryStatus::Idle);
    assert_eq!(r.cancelled_count(), 0);
    assert!(r.signal().is_none());
}

#[test]
fn reset_preserves_key_and_policies() {
    let mut r = test_resource_with_policies(
        "my-key",
        CachePolicy::StaleWhileRevalidate {
            ttl_ms: 5_000,
            stale_ms: 2_000,
        },
        RequestPolicy::LatestWins,
    );
    let mut s = seq();
    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "data", 200));

    r.reset();

    assert_eq!(r.key().as_str(), "my-key");
    assert_eq!(
        r.cache_policy(),
        CachePolicy::StaleWhileRevalidate {
            ttl_ms: 5_000,
            stale_ms: 2_000,
        }
    );
    assert_eq!(r.request_policy(), RequestPolicy::LatestWins);
}

#[test]
fn reset_cancels_signal() {
    let mut r = resource();
    let mut s = seq();

    let _ = r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    let sig = r.signal().unwrap().clone();

    r.reset();

    assert!(sig.is_cancelled(), "reset should cancel the signal");
    assert!(r.signal().is_none());
}

#[test]
fn reset_clears_diagnostic_counters() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "data", 200));
    r.increment_retry();
    r.increment_retry();

    // Replace request to bump cancelled_count
    let _ = begin(&mut r, &mut s, 1_500);

    r.reset();

    assert_eq!(r.cache_hits(), 0);
    assert_eq!(r.cancelled_count(), 0);
    assert_eq!(r.ignored_results(), 0);
    assert_eq!(r.retry_count(), 0);
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Retry counter increment and reset
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn retry_counter_increments_and_resets() {
    let mut r = resource();

    assert_eq!(r.retry_count(), 0);

    r.increment_retry();
    r.increment_retry();
    r.increment_retry();
    assert_eq!(r.retry_count(), 3);

    r.reset_retry_count();
    assert_eq!(r.retry_count(), 0);
}

#[test]
fn reset_clears_retry_count() {
    let mut r = resource();
    r.increment_retry();
    r.increment_retry();

    r.reset();

    assert_eq!(r.retry_count(), 0);
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Double begin_loading: LatestWins cancels old request
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn latest_wins_second_begin_replaces_active_request() {
    let mut r = resource();
    let mut s = seq();

    let (rid1, _) = begin(&mut r, &mut s, 100);
    let (rid2, _) = begin(&mut r, &mut s, 200);

    // rid1 is no longer active
    assert_ne!(r.active_request_id(), Some(rid1));
    assert_eq!(r.active_request_id(), Some(rid2));
    assert_eq!(
        r.cancelled_count(),
        1,
        "replaced request increments cancelled_count"
    );

    // Completing rid2 succeeds
    assert!(r.complete_current_success(rid2, "fresh", 300));
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"fresh"));
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Double begin_loading: IgnoreWhileLoading ignores second
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ignore_while_loading_rejects_second_request() {
    let mut r = resource_ignore();
    let mut s = seq();

    let result1 = r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    let rid1 = match result1 {
        QueryBeginResult::Started { request_id, .. } => request_id,
        _ => panic!("expected Started"),
    };

    let result2 = r.begin_request(&mut s, 200, QueryFetchMode::Normal);

    match result2 {
        QueryBeginResult::IgnoredWhileLoading {
            active_request_id,
        } => {
            assert_eq!(active_request_id, rid1);
        }
        _ => panic!(
            "expected IgnoredWhileLoading, got {:?}",
            result2
        ),
    }

    assert_eq!(r.active_request_id(), Some(rid1));
    assert_eq!(
        r.cancelled_count(),
        0,
        "no cancellation because request was ignored"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Stale data check (is_data_stale)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn is_data_stale_returns_true_when_loading_with_data() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "data", 200));

    let _ = begin(&mut r, &mut s, 1_500);

    assert!(r.is_data_stale());
}

#[test]
fn is_data_stale_returns_true_on_failure_with_data() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "data", 200));

    let (rid2, _) = begin(&mut r, &mut s, 1_500);
    assert!(r.complete_current_failure_with_data(
        rid2,
        "fallback",
        QueryError::response("err"),
        1_600
    ));

    assert!(r.is_data_stale(), "failure with data should be stale");
}

#[test]
fn is_data_stale_returns_false_on_success() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "data", 200));

    assert!(!r.is_data_stale());
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Optional success: None -> Idle (not Success)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn optional_success_none_sets_idle_not_success() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_optional_success(rid, None, 200));

    assert_eq!(
        r.status(),
        QueryStatus::Idle,
        "None data should produce Idle, not Success"
    );
    assert_eq!(r.data(), None);
}

#[test]
fn optional_success_some_sets_success() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_optional_success(rid, Some("data"), 200));

    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"data"));
}

#[test]
fn optional_success_none_clears_previous_data() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "old", 200));

    let (rid2, _) = begin(&mut r, &mut s, 1_500);
    assert!(r.complete_current_optional_success(rid2, None, 1_600));

    assert_eq!(r.status(), QueryStatus::Idle);
    assert_eq!(r.data(), None);
    assert_eq!(r.previous_data(), Some(&"old"));
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Cache short-circuit: CacheHit result
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cache_hit_returns_no_fetch_when_fresh() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "data", 200));

    // Within TTL (1000ms): should be a cache hit
    let result = r.begin_request(&mut s, 500, QueryFetchMode::Normal);

    assert_eq!(result, QueryBeginResult::CacheHit);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"data"));
    assert_eq!(r.cache_hits(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Stale-while-revalidate: StaleCacheHit result
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stale_while_revalidate_serves_stale_and_starts_background() {
    let mut r = test_resource_with_policies(
        "swr",
        CachePolicy::StaleWhileRevalidate {
            ttl_ms: 500,
            stale_ms: 1_000,
        },
        RequestPolicy::LatestWins,
    );
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "stale-data", 200));

    // At t=800: past TTL (500) but within stale window (500+1000=1500)
    let result = r.begin_request(&mut s, 800, QueryFetchMode::Normal);

    match result {
        QueryBeginResult::StaleCacheHit {
            request_id,
            status,
            replaced_request_id,
        } => {
            assert_eq!(status, QueryStatus::LoadingWithData);
            assert!(replaced_request_id.is_none(), "no prior request to replace");
            assert_eq!(r.active_request_id(), Some(request_id));
        }
        _ => panic!("expected StaleCacheHit, got {:?}", result),
    }

    assert_eq!(
        r.data(),
        Some(&"stale-data"),
        "stale data is still available"
    );
    assert_eq!(r.cache_hits(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 20. begin_request_with_id variant
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn begin_request_with_id_uses_provided_id() {
    let mut r = resource();
    let custom_id = RequestId::scoped(42, 7);

    let result = r.begin_request_with_id(Some(custom_id), 100, QueryFetchMode::Normal);

    match result {
        QueryBeginResult::Started { request_id, .. } => {
            assert_eq!(request_id, custom_id);
        }
        _ => panic!("expected Started"),
    }
    assert_eq!(r.active_request_id(), Some(custom_id));
}

#[test]
fn begin_request_with_id_none_uses_transient_sequencer() {
    let mut r = resource();

    let result = r.begin_request_with_id(None, 100, QueryFetchMode::Normal);

    match result {
        QueryBeginResult::Started { request_id, .. } => {
            // Transient sequencer starts at scope 1, sequence 1
            assert_eq!(request_id, RequestId::scoped(1, 1));
        }
        _ => panic!("expected Started"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 21. Force fetch mode bypasses cache
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn force_fetch_mode_bypasses_fresh_cache() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "data", 200));

    // Even though cache is fresh (t=300 < TTL 1000), force fetch should proceed
    let result = r.begin_request(&mut s, 300, QueryFetchMode::Force);

    match result {
        QueryBeginResult::Started { .. } => {}
        _ => panic!("expected Started with Force mode, got {:?}", result),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 22. Optimistic update: set_data / clear_data / rollback
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn set_data_saves_previous_for_rollback() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "real", 200));

    r.set_data("optimistic");

    assert_eq!(r.data(), Some(&"optimistic"));
    assert_eq!(r.previous_data(), Some(&"real"));

    assert!(r.rollback_to_previous());
    assert_eq!(r.data(), Some(&"real"));
    assert_eq!(r.previous_data(), None);
}

#[test]
fn clear_data_saves_for_rollback() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "real", 200));

    r.clear_data();

    assert_eq!(r.data(), None);
    assert_eq!(r.previous_data(), Some(&"real"));

    assert!(r.rollback_to_previous());
    assert_eq!(r.data(), Some(&"real"));
}

#[test]
fn rollback_returns_false_when_no_previous_data() {
    let mut r = resource();
    assert!(!r.rollback_to_previous());
}

// ═══════════════════════════════════════════════════════════════════════
// 23. Invalidate: clears timestamp but keeps data and active request
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn invalidate_clears_timestamp_but_retains_data_and_active_request() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "data", 200));

    let (_rid2, _) = begin(&mut r, &mut s, 1_500);

    r.invalidate();

    assert_eq!(r.data(), Some(&"data"));
    assert!(
        r.active_request_id().is_some(),
        "invalidate does not cancel active request"
    );
    assert_eq!(r.last_updated_at_ms(), None, "invalidate clears timestamp");
}

// ═══════════════════════════════════════════════════════════════════════
// 24. Initial data seeding
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn initial_data_seeds_idle_resource() {
    let mut r = resource();

    r.set_initial_data("seed", 100);

    assert_eq!(r.data(), Some(&"seed"));
    assert_eq!(r.initial_data(), Some(&"seed"));
    assert_eq!(r.last_updated_at_ms(), Some(100));
    assert_eq!(r.status(), QueryStatus::Idle);
}

#[test]
fn initial_data_ignored_when_not_idle() {
    let mut r = resource();
    let mut s = seq();

    let _ = begin(&mut r, &mut s, 100);

    r.set_initial_data("seed", 150);

    assert_eq!(r.data(), None, "should not seed data while loading");
    assert_eq!(r.initial_data(), None);
}

#[test]
fn initial_data_ignored_when_data_already_exists() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    assert!(r.complete_current_success(rid, "existing", 200));

    r.set_initial_data("seed", 250);

    assert_eq!(
        r.data(),
        Some(&"existing"),
        "existing data takes precedence"
    );
}

#[test]
fn reset_clears_initial_data() {
    let mut r = resource();
    r.set_initial_data("seed", 100);

    r.reset();

    assert!(r.initial_data().is_none());
    assert_eq!(r.data(), None);
}

// ═══════════════════════════════════════════════════════════════════════
// 25. Placeholder data
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn placeholder_data_falls_back_in_display_data() {
    let mut r = resource();
    r.set_placeholder_data(Some("placeholder"));

    assert_eq!(r.data(), None);
    assert_eq!(r.placeholder_data(), Some(&"placeholder"));
    assert_eq!(
        r.display_data(),
        Some(&"placeholder"),
        "display_data falls back to placeholder"
    );

    r.set_placeholder_data(None);
    assert_eq!(r.display_data(), None);
}

#[test]
fn reset_clears_placeholder_data() {
    let mut r = resource();
    r.set_placeholder_data(Some("ph"));

    r.reset();

    assert!(r.placeholder_data().is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 26. Multiple replacements: cancelled_count tracks all
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cancelled_count_increments_on_each_replacement() {
    let mut r = resource();
    let mut s = seq();

    let _ = begin(&mut r, &mut s, 100); // first
    let _ = begin(&mut r, &mut s, 200); // replaces first
    let _ = begin(&mut r, &mut s, 300); // replaces second

    assert_eq!(r.cancelled_count(), 2, "two requests were replaced");
}

#[test]
fn cancelled_count_includes_explicit_cancel() {
    let mut r = resource();
    let mut s = seq();

    let _ = begin(&mut r, &mut s, 100);
    assert!(r.cancel(QueryError::cancelled("abort")));

    assert_eq!(r.cancelled_count(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 27. Two-phase protocol: accept + complete via guard
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn accept_then_complete_success_via_guard() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);

    let guard = r
        .accept_current_request(rid)
        .expect("should accept current request");
    assert!(
        r.active_request_id().is_none(),
        "accept clears active_request_id"
    );

    r.complete_success(guard, "data", 200);

    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"data"));
    assert_eq!(r.last_updated_at_ms(), Some(200));
}

#[test]
fn accept_then_complete_failure_via_guard() {
    let mut r = resource();
    let mut s = seq();

    let (rid, _) = begin(&mut r, &mut s, 100);
    let guard = r.accept_current_request(rid).expect("should accept");

    r.complete_failure(guard, QueryError::transport("net error"), 200);

    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(err_str(&r), Some("transport error: net error".to_string()));
}

// ═══════════════════════════════════════════════════════════════════════
// 28. is_current_request
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn is_current_request_matches_active() {
    let mut r = resource();
    let mut s = seq();

    let (rid1, _) = begin(&mut r, &mut s, 100);
    assert!(r.is_current_request(rid1));

    let (rid2, _) = begin(&mut r, &mut s, 200);
    assert!(!r.is_current_request(rid1), "rid1 is stale");
    assert!(r.is_current_request(rid2), "rid2 is current");
}

// ═══════════════════════════════════════════════════════════════════════
// 29. Full lifecycle: Idle -> LoadingEmpty -> Success -> LoadingWithData
//     -> Success (updated) -> LoadingWithData -> Cancel -> Rollback
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn full_lifecycle_round_trip() {
    let mut r = resource();
    let mut s = seq();

    // Phase 1: Idle -> LoadingEmpty
    assert_eq!(r.status(), QueryStatus::Idle);
    let (rid1, status1) = begin(&mut r, &mut s, 100);
    assert_eq!(status1, QueryStatus::LoadingEmpty);

    // Phase 2: LoadingEmpty -> Success
    assert!(r.complete_current_success(rid1, "v1", 200));
    assert_eq!(r.status(), QueryStatus::Success);

    // Phase 3: Success -> LoadingWithData
    let (rid2, status2) = begin(&mut r, &mut s, 1_500);
    assert_eq!(status2, QueryStatus::LoadingWithData);
    assert_eq!(r.data(), Some(&"v1"));

    // Phase 4: LoadingWithData -> Success (updated)
    assert!(r.complete_current_success(rid2, "v2", 1_600));
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"v2"));
    assert_eq!(r.previous_data(), Some(&"v1"));

    // Phase 5: Success -> LoadingWithData -> Cancel
    // Use t=3_000 which is beyond TTL (1_000ms from t=1_600)
    let (_rid3, _) = begin(&mut r, &mut s, 3_000);
    assert_eq!(r.status(), QueryStatus::LoadingWithData);
    assert!(r.cancel(QueryError::cancelled("manual")));
    assert_eq!(r.status(), QueryStatus::Cancelled);
    assert_eq!(r.data(), None);
    assert_eq!(r.previous_data(), Some(&"v2"));

    // Phase 6: Rollback
    assert!(r.rollback_to_previous());
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"v2"));

    // Phase 7: Full reset
    r.reset();
    assert_eq!(r.status(), QueryStatus::Idle);
    assert_eq!(r.data(), None);
    assert_eq!(r.cancelled_count(), 0);
}
