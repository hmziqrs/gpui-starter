//! State-transition invariant tests.
//!
//! Table-driven verification that status and data are never inconsistent after
//! any state transition.

use crate::core::*;
use crate::tests::test_support::*;

#[test]
fn invariant_initial_state_is_consistent() {
    let r = fresh_resource();
    assert_eq!(r.status(), QueryStatus::Idle);
    assert!(r.data().is_none(), "Idle => data must be None");
    assert!(r.error().is_none(), "Idle => error must be None");
    assert!(r.active_request_id().is_none());
}

#[test]
fn invariant_after_begin_loading_empty() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    assert_eq!(r.status(), QueryStatus::LoadingEmpty);
    assert!(r.data().is_none(), "LoadingEmpty => data must be None");
    assert!(r.error().is_none(), "begin_request clears error");
    assert!(r.active_request_id().is_some());
    assert!(r.signal().is_some());
}

#[test]
fn invariant_after_begin_loading_with_data() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    // First fetch succeeds to get data.
    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_success(rid1, "data1", 200);

    // Second fetch: resource has data, so status should be LoadingWithData.
    let rid2 = begin_request_id(&mut r, &mut s, 300, QueryFetchMode::Normal);
    assert_eq!(r.status(), QueryStatus::LoadingWithData);
    // Data should still be present during refetch (optimistic).
    assert!(r.data().is_some(), "LoadingWithData => data should still be present");
    assert_eq!(r.data(), Some(&"data1"), "data preserved during refetch");
    assert!(r.error().is_none(), "begin_request clears error");
    assert_eq!(r.active_request_id(), Some(rid2));

    // Complete the refetch — old data should become previous_data.
    r.complete_current_success(rid2, "data2", 400);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"data2"));
    assert_eq!(r.previous_data(), Some(&"data1"));
}

#[test]
fn invariant_after_complete_success() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_success(rid, "result", 200);

    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"result"), "Success => data must be Some");
    assert!(r.error().is_none(), "Success => error must be None");
    assert!(r.active_request_id().is_none(), "completed => no active request");
    assert!(r.signal().is_some()); // signal remains but request is done
}

#[test]
fn invariant_after_complete_failure() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_failure(rid, QueryError::response("fail"), 200);

    assert_eq!(r.status(), QueryStatus::Failure);
    assert!(r.data().is_none(), "Failure from LoadingEmpty => data must be None");
    assert!(r.error().is_some(), "Failure => error must be Some");
    assert!(r.active_request_id().is_none(), "completed => no active request");
}

#[test]
fn invariant_after_complete_failure_from_loading_with_data() {
    // When a refetch fails (apply_failure), data is RETAINED (not cleared).
    // apply_failure only sets status=Failure and error; it does NOT touch data.
    // This matches TanStack Query behavior where a failed refetch keeps stale data.
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    // First fetch succeeds.
    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_success(rid1, "original", 200);

    // Second fetch fails (refetch).
    let rid2 = begin_request_id(&mut r, &mut s, 300, QueryFetchMode::Normal);
    r.complete_current_failure(rid2, QueryError::transport("timeout"), 400);

    assert_eq!(r.status(), QueryStatus::Failure);
    assert!(r.error().is_some(), "Failure => error must be Some");
    assert!(r.active_request_id().is_none());
    // Key invariant: apply_failure does NOT clear data or set previous_data.
    // The data from before the refetch is retained in-place.
    assert_eq!(r.data(), Some(&"original"), "apply_failure retains data in-place");
    assert!(r.previous_data().is_none(), "apply_failure does NOT set previous_data");
}

#[test]
fn invariant_after_cancel_from_loading_empty() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    r.begin_request(&mut s, 100, QueryFetchMode::Normal);

    let cancelled = r.cancel(QueryError::cancelled("abort"));
    assert!(cancelled, "cancel should return true when request is active");
    assert_eq!(r.status(), QueryStatus::Cancelled);
    assert!(r.data().is_none(), "Cancelled from LoadingEmpty => data must be None");
    assert!(r.error().is_some(), "Cancelled => error must be Some");
    assert!(r.active_request_id().is_none(), "cancelled => no active request");
}

#[test]
fn invariant_after_cancel_from_loading_with_data() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    // First fetch succeeds.
    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_success(rid1, "data", 200);

    // Start a refetch, then cancel.
    r.begin_request(&mut s, 300, QueryFetchMode::Normal);
    assert_eq!(r.status(), QueryStatus::LoadingWithData);
    assert_eq!(r.data(), Some(&"data"));

    let cancelled = r.cancel(QueryError::cancelled("abort"));
    assert!(cancelled);
    assert_eq!(r.status(), QueryStatus::Cancelled);
    assert!(r.data().is_none(), "cancel clears data (saved to previous_data)");
    assert!(r.error().is_some());
    assert_eq!(r.previous_data(), Some(&"data"), "cancel saves data to previous_data for rollback");
}

#[test]
fn invariant_cancel_returns_false_when_no_active_request() {
    let mut r = fresh_resource();
    assert!(!r.cancel(QueryError::cancelled("noop")));
    assert_eq!(r.status(), QueryStatus::Idle);
}

#[test]
fn invariant_after_reset() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_success(rid, "data", 200);
    r.set_placeholder_data(Some("placeholder"));
    r.increment_retry();

    r.reset();

    assert_eq!(r.status(), QueryStatus::Idle);
    assert!(r.data().is_none(), "reset clears data");
    assert!(r.error().is_none(), "reset clears error");
    assert!(r.active_request_id().is_none());
    assert!(r.signal().is_none());
    assert!(r.placeholder_data().is_none());
    assert!(r.previous_data().is_none());
    assert!(r.initial_data().is_none());
    assert_eq!(r.cache_hits(), 0);
    assert_eq!(r.cancelled_count(), 0);
    assert_eq!(r.ignored_results(), 0);
    assert_eq!(r.retry_count(), 0);
    // Policies are preserved.
    assert_eq!(r.cache_policy(), CachePolicy::NoCache);
    assert_eq!(r.request_policy(), RequestPolicy::LatestWins);
}

#[test]
fn invariant_complete_success_optional_none_yields_idle() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    // complete_success_optional with None should yield Idle, not Success.
    let guard = r.accept_current_request(rid).unwrap();
    r.complete_success_optional(guard, None, 200);

    assert_eq!(r.status(), QueryStatus::Idle, "None data => Idle (not Success)");
    assert!(r.data().is_none(), "Idle => data must be None");
    assert!(r.error().is_none());
}

#[test]
fn invariant_complete_success_optional_some_yields_success() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    let guard = r.accept_current_request(rid).unwrap();
    r.complete_success_optional(guard, Some("data"), 200);

    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"data"));
}

#[test]
fn invariant_complete_failure_with_data() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    let guard = r.accept_current_request(rid).unwrap();
    r.complete_failure_with_data(guard, "fallback", QueryError::response("partial"), 200);

    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.data(), Some(&"fallback"), "Failure with data => data must be Some");
    assert!(r.error().is_some(), "Failure => error must be Some");
}

#[test]
fn invariant_stale_accept_rejected() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();
    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    // Start a second request (replaces the first under LatestWins).
    let rid2 = begin_request_id(&mut r, &mut s, 200, QueryFetchMode::Normal);
    // rid1 is now stale. accept_current_request should return None.
    assert!(r.accept_current_request(rid1).is_none(), "stale request should be rejected");
    assert_eq!(r.ignored_results(), 1);

    // rid2 is current. accept_current_request should succeed.
    assert!(r.accept_current_request(rid2).is_some(), "current request should be accepted");
}

// --- Table-driven: all transitions from each starting state ---------------

/// Enumerate all possible state transitions and verify invariants.
#[test]
fn table_driven_all_transitions_from_idle() {
    let mut r = fresh_resource();
    assert_eq!(r.status(), QueryStatus::Idle);

    // Transition: Idle -> LoadingEmpty (begin_request)
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    assert_eq!(r.status(), QueryStatus::LoadingEmpty);
    assert!(r.data().is_none());

    // Transition: LoadingEmpty -> Success (complete_success)
    r.complete_current_success(rid, "data", 200);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"data"));
    assert!(r.error().is_none());

    // Transition: Success -> LoadingWithData (begin_request again)
    let rid2 = begin_request_id(&mut r, &mut s, 300, QueryFetchMode::Normal);
    assert_eq!(r.status(), QueryStatus::LoadingWithData);
    assert_eq!(r.data(), Some(&"data"), "LoadingWithData preserves data");

    // Transition: LoadingWithData -> Failure (complete_failure)
    r.complete_current_failure(rid2, QueryError::response("fail"), 400);
    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.data(), Some(&"data"), "apply_failure retains data in-place");
    assert!(r.previous_data().is_none(), "apply_failure does NOT set previous_data");

    // Transition: Failure -> LoadingWithData (begin_request when data is present)
    let _rid3 = begin_request_id(&mut r, &mut s, 500, QueryFetchMode::Normal);
    assert_eq!(r.status(), QueryStatus::LoadingWithData, "data present => LoadingWithData even after Failure");
    assert!(r.error().is_none(), "begin_request clears error");

    // Transition: LoadingEmpty -> Cancelled (cancel)
    r.cancel(QueryError::cancelled("abort"));
    assert_eq!(r.status(), QueryStatus::Cancelled);
    assert!(r.data().is_none());
    assert!(r.error().is_some());

    // Transition: Cancelled -> Idle (reset)
    r.reset();
    assert_eq!(r.status(), QueryStatus::Idle);
    assert!(r.data().is_none());
    assert!(r.error().is_none());
}

#[test]
fn table_driven_cancel_from_every_loading_state() {
    // Cancel from LoadingEmpty.
    {
        let mut r = fresh_resource();
        let mut s = test_sequencer();
        r.begin_request(&mut s, 100, QueryFetchMode::Normal);
        assert_eq!(r.status(), QueryStatus::LoadingEmpty);
        r.cancel(QueryError::cancelled("abort"));
        assert_eq!(r.status(), QueryStatus::Cancelled);
        assert!(r.data().is_none());
        assert!(r.error().is_some());
    }

    // Cancel from LoadingWithData.
    {
        let mut r = fresh_resource();
        let mut s = test_sequencer();
        let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
        r.complete_current_success(rid, "data", 200);
        r.begin_request(&mut s, 300, QueryFetchMode::Normal);
        assert_eq!(r.status(), QueryStatus::LoadingWithData);
        r.cancel(QueryError::cancelled("abort"));
        assert_eq!(r.status(), QueryStatus::Cancelled);
        assert!(r.data().is_none(), "cancel clears data (saves to previous_data)");
        assert_eq!(r.previous_data(), Some(&"data"));
    }
}

#[test]
fn table_driven_rollback_from_every_state() {
    // rollback_to_previous only works when previous_data is set.
    // It sets status to Success and restores data.

    // From Success with previous_data (after a second successful fetch).
    {
        let mut r = fresh_resource();
        let mut s = test_sequencer();
        let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
        r.complete_current_success(rid1, "v1", 200);
        let rid2 = begin_request_id(&mut r, &mut s, 300, QueryFetchMode::Normal);
        r.complete_current_success(rid2, "v2", 400);
        assert_eq!(r.previous_data(), Some(&"v1"));

        let rolled_back = r.rollback_to_previous();
        assert!(rolled_back);
        assert_eq!(r.status(), QueryStatus::Success, "rollback sets Success");
        assert_eq!(r.data(), Some(&"v1"), "rollback restores previous data");
    }

    // From Cancelled with previous_data (cancel saves to previous_data).
    {
        let mut r = fresh_resource();
        let mut s = test_sequencer();
        let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
        r.complete_current_success(rid1, "v1", 200);
        r.begin_request(&mut s, 300, QueryFetchMode::Normal);
        r.cancel(QueryError::cancelled("abort"));
        assert_eq!(r.previous_data(), Some(&"v1"));

        let rolled_back = r.rollback_to_previous();
        assert!(rolled_back);
        assert_eq!(r.status(), QueryStatus::Success);
        assert_eq!(r.data(), Some(&"v1"));
    }

    // From Failure with data retained (apply_failure does NOT clear data or set previous_data).
    // To have previous_data from a failure scenario, we need to use the optimistic update path.
    {
        let mut r = fresh_resource();
        let mut s = test_sequencer();
        let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
        r.complete_current_success(rid1, "v1", 200);
        // Optimistic update sets previous_data.
        r.set_data("v2_optimistic");
        assert_eq!(r.data(), Some(&"v2_optimistic"));
        assert_eq!(r.previous_data(), Some(&"v1"));

        // Now rollback.
        let rolled_back = r.rollback_to_previous();
        assert!(rolled_back);
        assert_eq!(r.status(), QueryStatus::Success);
        assert_eq!(r.data(), Some(&"v1"));
    }

    // From Idle with no previous_data => rollback returns false.
    {
        let mut r = fresh_resource();
        assert!(!r.rollback_to_previous(), "no previous_data => rollback fails");
        assert_eq!(r.status(), QueryStatus::Idle);
    }
}
