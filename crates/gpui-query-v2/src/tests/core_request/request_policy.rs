//! Tests for RequestPolicy variants and QuerySignal lifecycle.
//!
//! Covers:
//! - RequestPolicy: LatestWins cancels previous, IgnoreWhileLoading keeps previous
//! - QuerySignal: creation per request, cancellation propagation across clones
//! - Scope-based rejection: request id from different scope is rejected

use crate::core::{
    CachePolicy, QueryBeginResult, QueryFetchMode, QueryResource, QuerySignal, QueryStatus,
    RequestId, RequestPolicy,
};
use crate::tests::test_support::{
    TEST_NOW_MS, assert_status, test_resource_with_policies, test_sequencer,
};

// ═══════════════════════════════════════════════════════════════════════════
// RequestPolicy: LatestWins cancels previous
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn latest_wins_cancels_previous_request_and_signal() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    // Start request 1, capture its signal
    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let old_id = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };
    let old_signal = resource.signal().unwrap().clone();
    assert!(!old_signal.is_cancelled());

    // Start request 2 — should cancel request 1's signal
    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let new_id = match result {
        QueryBeginResult::Started {
            request_id,
            replaced_request_id,
            ..
        } => {
            assert_eq!(replaced_request_id, Some(old_id));
            request_id
        }
        other => panic!("expected Started, got {:?}", other),
    };

    // Old signal should be cancelled
    assert!(
        old_signal.is_cancelled(),
        "replaced request's signal should be cancelled"
    );

    // New signal should not be cancelled
    let new_signal = resource.signal().unwrap();
    assert!(!new_signal.is_cancelled());

    // Old request id should not be accepted
    assert!(resource.accept_current_request(old_id).is_none());
    // New request id should be accepted
    assert!(resource.accept_current_request(new_id).is_some());

    assert_eq!(resource.cancelled_count(), 1);
}

#[test]
fn latest_wins_increments_cancelled_count_for_each_replacement() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    for _ in 0..5 {
        resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    }
    // First request: 0 cancellations; each subsequent: +1
    assert_eq!(resource.cancelled_count(), 4);
}

// ═══════════════════════════════════════════════════════════════════════════
// RequestPolicy: IgnoreWhileLoading keeps previous
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ignore_while_loading_rejects_new_request_when_loading() {
    let mut resource: QueryResource<&str> = test_resource_with_policies(
        "key",
        CachePolicy::NoCache,
        RequestPolicy::IgnoreWhileLoading,
    );
    let mut seq = test_sequencer();

    // First request starts normally
    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let first_id = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };
    assert_status(&resource, QueryStatus::LoadingEmpty);

    // Second request should be ignored
    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    match result {
        QueryBeginResult::IgnoredWhileLoading { active_request_id } => {
            assert_eq!(active_request_id, first_id);
        }
        other => panic!("expected IgnoredWhileLoading, got {:?}", other),
    }

    // Active request is still the first one
    assert_eq!(resource.active_request_id(), Some(first_id));
    assert_eq!(
        resource.cancelled_count(),
        0,
        "no cancellations should occur"
    );
}

#[test]
fn ignore_while_loading_allows_new_request_after_completion() {
    let mut resource: QueryResource<&str> = test_resource_with_policies(
        "key",
        CachePolicy::NoCache,
        RequestPolicy::IgnoreWhileLoading,
    );
    let mut seq = test_sequencer();

    // Start and complete first request
    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let first_id = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };
    let completed = resource.complete_current_success(first_id, "data", TEST_NOW_MS);
    assert!(completed);
    assert_status(&resource, QueryStatus::Success);

    // Now a new request should be allowed
    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    assert!(
        matches!(result, QueryBeginResult::Started { .. }),
        "new request should start after previous completed"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Signal creation per request
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn each_request_gets_a_fresh_signal() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    // Request 1
    resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let signal_1 = resource.signal().unwrap().clone();

    // Complete request 1 — completion does not clear the signal;
    // it remains until replaced by the next begin_loading call.
    let id_1 = RequestId::scoped(1, 1);
    resource.complete_current_success(id_1, "result", TEST_NOW_MS);

    // Request 2 — begin_loading cancels the old signal and creates a new one
    resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let signal_2 = resource.signal().unwrap().clone();

    assert_ne!(
        signal_1, signal_2,
        "each request should get a distinct signal"
    );
    assert!(
        signal_1.is_cancelled(),
        "old signal should be cancelled when replaced"
    );
    assert!(
        !signal_2.is_cancelled(),
        "new signal should not be cancelled"
    );
}

#[test]
fn signal_cancellation_propagates_to_all_clones() {
    let signal = QuerySignal::new();
    let clone1 = signal.clone();
    let clone2 = signal.clone();

    assert!(!signal.is_cancelled());
    assert!(!clone1.is_cancelled());
    assert!(!clone2.is_cancelled());

    clone1.cancel();

    assert!(signal.is_cancelled());
    assert!(clone1.is_cancelled());
    assert!(clone2.is_cancelled());
}

#[test]
fn signal_is_not_cancelled_on_creation() {
    let signal = QuerySignal::new();
    assert!(!signal.is_cancelled());
    let default = QuerySignal::default();
    assert!(!default.is_cancelled());
}

// ═══════════════════════════════════════════════════════════════════════════
// Scope-based rejection: request id from different scope is rejected
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_id_from_different_scope_is_rejected() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    // Start a request — scope 1
    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let active_id = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    // Manually craft a request id in a different scope
    let fake_id = RequestId::scoped(999, 1);
    let result = resource.accept_current_request(fake_id);
    assert!(
        result.is_none(),
        "request id from different scope should be rejected"
    );
    // The actual active request should still be accepted
    assert_eq!(resource.active_request_id(), Some(active_id));
    assert_eq!(resource.ignored_results(), 1);
}
