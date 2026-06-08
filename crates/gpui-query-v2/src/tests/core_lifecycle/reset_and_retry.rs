//! Reset, retry counter, and stale request rejection tests (sections 11-13).
//!
//! Covers: stale request rejection, reset from every state, retry counter.

use crate::core::*;
use crate::tests::test_support::test_resource_with_policies;
use crate::tests::core_lifecycle::transitions::*;

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
