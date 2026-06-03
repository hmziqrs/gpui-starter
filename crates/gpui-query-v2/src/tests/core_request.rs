//! Comprehensive tests for REQUEST MANAGEMENT in gpui-query-v2.
//!
//! Covers:
//! - RequestId: construction, fields, ordering, label, equality
//! - RequestSequencer: monotonicity, scope advancement, wrapping at u64::MAX
//! - RequestGuard: proof-of-ownership, scope-based rejection, into_request_id
//! - RequestPolicy: LatestWins cancels previous, IgnoreWhileLoading keeps previous
//! - QuerySignal: creation per request, cancellation propagation across clones
//! - begin_request_with_id variant

use crate::core::{
    CachePolicy, QueryBeginResult, QueryFetchMode, QueryResource, QuerySignal, QueryStatus,
    RequestId, RequestPolicy, RequestSequencer,
};
use crate::tests::test_support::{
    assert_status, test_resource_with_policies, test_sequencer, TEST_NOW_MS,
};

// ═══════════════════════════════════════════════════════════════════════════
// RequestId basics
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_id_scoped_accesses_scope_and_value() {
    let id = RequestId::scoped(3, 7);
    assert_eq!(id.scope_id(), 3);
    assert_eq!(id.value(), 7);
}

#[test]
fn request_id_label_format() {
    let id = RequestId::scoped(42, 99);
    assert_eq!(id.label(), "42:99");
}

#[test]
fn request_id_equality_requires_both_fields() {
    let a = RequestId::scoped(1, 10);
    let b = RequestId::scoped(1, 10);
    let c = RequestId::scoped(2, 10);
    let d = RequestId::scoped(1, 20);
    assert_eq!(a, b, "same scope and sequence should be equal");
    assert_ne!(a, c, "different scope should not be equal");
    assert_ne!(a, d, "different sequence should not be equal");
}

#[test]
fn request_id_ordering_is_lexicographic() {
    let a = RequestId::scoped(1, 100);
    let b = RequestId::scoped(2, 1);
    let c = RequestId::scoped(1, 200);
    // scope is compared first
    assert!(a < b, "scope 1 < scope 2 regardless of sequence");
    // same scope, sequence compared
    assert!(a < c, "scope 1 seq 100 < scope 1 seq 200");
}

// ═══════════════════════════════════════════════════════════════════════════
// RequestSequencer monotonicity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sequencer_starts_at_scope_1_seq_1() {
    let mut seq = RequestSequencer::new();
    let id = seq.next_request();
    assert_eq!(id.scope_id(), 1);
    assert_eq!(id.value(), 1);
}

#[test]
fn sequencer_produces_monotonically_increasing_ids() {
    let mut seq = RequestSequencer::new();
    let mut prev = seq.next_request();
    for _ in 0..50 {
        let curr = seq.next_request();
        assert!(
            curr > prev,
            "expected {:?} > {:?} but ordering violated",
            curr,
            prev
        );
        prev = curr;
    }
}

#[test]
fn sequencer_default_matches_new() {
    let default = RequestSequencer::default();
    let new = RequestSequencer::new();
    assert_eq!(default, new);
}

#[test]
fn sequencer_is_current_scope_tracks_scope_changes() {
    let mut seq = RequestSequencer::new();
    let id_in_scope = seq.next_request();
    assert!(seq.is_current_scope(id_in_scope));

    seq.advance_scope();
    assert!(
        !seq.is_current_scope(id_in_scope),
        "after advance_scope, old ids should not match"
    );

    let id_in_new_scope = seq.next_request();
    assert!(seq.is_current_scope(id_in_new_scope));
}

// ═══════════════════════════════════════════════════════════════════════════
// RequestSequencer wrapping (u64::MAX → scope advance)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sequencer_advances_scope_when_sequence_reaches_max() {
    let mut seq = RequestSequencer {
        scope_id: 5,
        next_request_id: u64::MAX,
    };
    // This call should produce scope 5, seq u64::MAX and then advance scope.
    let id = seq.next_request();
    assert_eq!(id.scope_id(), 5);
    assert_eq!(id.value(), u64::MAX);

    // After advancing, the next id should be in scope 6, seq 1.
    let next_id = seq.next_request();
    assert_eq!(next_id.scope_id(), 6, "scope should have advanced to 6");
    assert_eq!(
        next_id.value(),
        1,
        "sequence should reset to 1 after scope advance"
    );
}

#[test]
fn sequencer_scope_id_wraps_to_1_on_overflow() {
    let mut seq = RequestSequencer {
        scope_id: u64::MAX,
        next_request_id: u64::MAX,
    };
    // First call returns (u64::MAX, u64::MAX) and then advances scope.
    let id = seq.next_request();
    assert_eq!(id.scope_id(), u64::MAX);
    assert_eq!(id.value(), u64::MAX);

    // scope_id.checked_add(1) overflows -> wraps to 1
    let next_id = seq.next_request();
    assert_eq!(
        next_id.scope_id(),
        1,
        "scope_id should wrap to 1 on u64::MAX overflow"
    );
    assert_eq!(next_id.value(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// RequestGuard: proof-of-ownership (obtained via accept_current_request)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn guard_holds_the_correct_request_id() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let rid = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    let guard = resource.accept_current_request(rid).unwrap();
    assert_eq!(guard.request_id(), rid);
}

#[test]
fn guard_into_request_id_consumes_guard() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let rid = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    let guard = resource.accept_current_request(rid).unwrap();
    let extracted = guard.into_request_id();
    assert_eq!(extracted, rid);
    // guard is consumed — calling guard.request_id() here would not compile.
}

#[test]
fn accept_current_request_returns_guard_for_active_request() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let request_id = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    let guard = resource
        .accept_current_request(request_id)
        .expect("should accept the active request");
    assert_eq!(guard.request_id(), request_id);
}

#[test]
fn accept_current_request_rejects_stale_request_id() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    // Start request 1
    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let old_id = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    // Start request 2 — replaces request 1 under LatestWins
    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let _new_id = match result {
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

    // Trying to accept the old request should fail
    let result = resource.accept_current_request(old_id);
    assert!(
        result.is_none(),
        "stale request id should be rejected, but got a guard"
    );
    assert_eq!(resource.ignored_results(), 1);
}

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

    assert_ne!(signal_1, signal_2, "each request should get a distinct signal");
    assert!(signal_1.is_cancelled(), "old signal should be cancelled when replaced");
    assert!(!signal_2.is_cancelled(), "new signal should not be cancelled");
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

// ═══════════════════════════════════════════════════════════════════════════
// Two-phase completion via guard
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn complete_success_consumes_guard_and_sets_data() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let rid = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    let guard = resource.accept_current_request(rid).unwrap();
    resource.complete_success(guard, "result", TEST_NOW_MS);

    assert_eq!(resource.data(), Some(&"result"));
    assert_status(&resource, QueryStatus::Success);
    assert_eq!(resource.active_request_id(), None);
}

#[test]
fn complete_failure_consumes_guard_and_sets_error() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let rid = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    let guard = resource.accept_current_request(rid).unwrap();
    resource.complete_failure(guard, "network error", TEST_NOW_MS);

    assert!(resource.data().is_none());
    assert_status(&resource, QueryStatus::Failure);
    assert!(resource.error().is_some());
}

#[test]
fn complete_convenience_method_rejects_stale_id() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let mut seq = test_sequencer();

    // Request 1
    let result = resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);
    let old_id = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };

    // Request 2 replaces request 1
    resource.begin_request(&mut seq, TEST_NOW_MS, QueryFetchMode::Normal);

    // Trying to complete old request should fail
    let completed = resource.complete_current_success(old_id, "stale data", TEST_NOW_MS);
    assert!(!completed, "stale request should not be completed");
    assert!(
        resource.data().is_none(),
        "data should remain unset after stale completion attempt"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// begin_request_with_id variant
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn begin_request_with_id_uses_provided_id() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);
    let custom_id = RequestId::scoped(99, 7);

    let result =
        resource.begin_request_with_id(Some(custom_id), TEST_NOW_MS, QueryFetchMode::Normal);
    let rid = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };
    assert_eq!(rid, custom_id);
}

#[test]
fn begin_request_with_id_none_falls_back_to_transient_sequencer() {
    let mut resource: QueryResource<&str> =
        test_resource_with_policies("key", CachePolicy::NoCache, RequestPolicy::LatestWins);

    let result = resource.begin_request_with_id(None, TEST_NOW_MS, QueryFetchMode::Normal);
    let rid = match result {
        QueryBeginResult::Started { request_id, .. } => request_id,
        other => panic!("expected Started, got {:?}", other),
    };
    // Transient sequencer starts at scope 1, seq 1
    assert_eq!(rid.scope_id(), 1);
    assert_eq!(rid.value(), 1);
}
