//! Concurrency / two-phase completion protocol tests.
//!
//! Verify that the two-phase completion protocol maintains invariants even when
//! requests are interleaved. Also covers signal, display_data, initial_data,
//! and is_data_stale tests.

use crate::core::*;
use crate::tests::test_support::*;

#[test]
fn two_phase_protocol_accept_then_complete_is_consistent() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);

    // Phase 1: accept
    let guard = r
        .accept_current_request(rid)
        .expect("should accept current request");
    assert!(
        r.active_request_id().is_none(),
        "accept clears active_request_id"
    );

    // Phase 2: complete with success
    r.complete_success(guard, "result", 200);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"result"));
}

#[test]
fn two_phase_stale_accept_then_complete_does_not_corrupt() {
    // Begin two requests, try to complete the first (stale) — it should be
    // rejected, and the second should complete successfully.
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    let rid2 = begin_request_id(&mut r, &mut s, 200, QueryFetchMode::Normal);

    // rid1 is stale. complete_current_success should return false.
    assert!(!r.complete_current_success(rid1, "stale_data", 300));
    assert_eq!(r.ignored_results(), 1);

    // rid2 is current. complete_current_success should return true.
    assert!(r.complete_current_success(rid2, "fresh_data", 400));
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"fresh_data"));
}

#[test]
fn concurrent_replacements_increment_cancelled_count() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    // Each replacement increments cancelled_count.
    let _ = r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    assert_eq!(r.cancelled_count(), 0);
    let _ = r.begin_request(&mut s, 200, QueryFetchMode::Normal);
    assert_eq!(r.cancelled_count(), 1);
    let _ = r.begin_request(&mut s, 300, QueryFetchMode::Normal);
    assert_eq!(r.cancelled_count(), 2);
    let _ = r.begin_request(&mut s, 400, QueryFetchMode::Normal);
    assert_eq!(r.cancelled_count(), 3);
}

#[test]
fn ignore_while_loading_rejects_concurrent_requests() {
    let mut r: QueryResource<&str> = QueryResource::new(
        "ignore-test",
        CachePolicy::NoCache,
        RequestPolicy::IgnoreWhileLoading,
    );
    let mut s = test_sequencer();

    // First request starts.
    let rid1 = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    assert_eq!(r.active_request_id(), Some(rid1));

    // Second request is ignored.
    let result = r.begin_request(&mut s, 200, QueryFetchMode::Normal);
    match result {
        QueryBeginResult::IgnoredWhileLoading { active_request_id } => {
            assert_eq!(active_request_id, rid1);
        }
        _ => panic!("expected IgnoredWhileLoading, got {:?}", result),
    }
    assert_eq!(
        r.active_request_id(),
        Some(rid1),
        "active request should not change"
    );
    assert_eq!(r.cancelled_count(), 0, "no cancellation on ignore");

    // Complete the first request.
    r.complete_current_success(rid1, "data", 300);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.data(), Some(&"data"));
}

#[test]
fn signal_cancelled_on_replacement() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    let _ = r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    let signal1 = r.signal().unwrap().clone();
    assert!(!signal1.is_cancelled());

    // Replace the request — the old signal should be cancelled.
    let _ = r.begin_request(&mut s, 200, QueryFetchMode::Normal);
    assert!(
        signal1.is_cancelled(),
        "old signal should be cancelled on replacement"
    );
    let signal2 = r.signal().unwrap().clone();
    assert!(
        !signal2.is_cancelled(),
        "new signal should not be cancelled"
    );
    assert_ne!(signal1, signal2, "signals should be different");
}

#[test]
fn signal_cancelled_on_explicit_cancel() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    let signal = r.signal().unwrap().clone();
    assert!(!signal.is_cancelled());

    r.cancel(QueryError::cancelled("abort"));
    assert!(
        signal.is_cancelled(),
        "signal should be cancelled after explicit cancel"
    );
}

#[test]
fn signal_cancelled_on_reset() {
    let mut r = fresh_resource();
    let mut s = test_sequencer();

    r.begin_request(&mut s, 100, QueryFetchMode::Normal);
    let signal = r.signal().unwrap().clone();
    assert!(!signal.is_cancelled());

    r.reset();
    assert!(signal.is_cancelled(), "signal should be cancelled on reset");
    assert!(r.signal().is_none(), "no signal after reset");
}

#[test]
fn display_data_falls_back_to_placeholder() {
    let mut r = fresh_resource();
    assert!(r.display_data().is_none());

    r.set_placeholder_data(Some("placeholder"));
    assert_eq!(r.display_data(), Some(&"placeholder"));

    // When data is present, data takes priority.
    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_success(rid, "real_data", 200);
    assert_eq!(
        r.display_data(),
        Some(&"real_data"),
        "data takes priority over placeholder"
    );
}

#[test]
fn initial_data_seeded_when_idle() {
    let mut r = fresh_resource();
    r.set_initial_data("seeded", 500);
    assert_eq!(
        r.data(),
        Some(&"seeded"),
        "initial data should populate data"
    );
    assert_eq!(r.last_updated_at_ms(), Some(500));
    assert!(r.initial_data().is_some());

    // Seeding again while not Idle+None should be a no-op.
    r.set_initial_data("ignored", 600);
    assert_eq!(r.data(), Some(&"seeded"), "second seed should be ignored");
}

#[test]
fn initial_data_cleared_on_reset() {
    let mut r = fresh_resource();
    r.set_initial_data("seeded", 500);
    assert!(r.initial_data().is_some());
    r.reset();
    assert!(r.initial_data().is_none(), "initial_data cleared on reset");
    assert!(r.data().is_none(), "data cleared on reset");
}

#[test]
fn is_data_stale_heuristic() {
    let mut r = fresh_resource();
    assert!(!r.is_data_stale(), "no data => not stale");

    let mut s = test_sequencer();
    let rid = begin_request_id(&mut r, &mut s, 100, QueryFetchMode::Normal);
    r.complete_current_success(rid, "data", 200);
    assert!(!r.is_data_stale(), "Success with data => not stale");

    // Start a refetch — data is stale (LoadingWithData).
    r.begin_request(&mut s, 300, QueryFetchMode::Normal);
    assert_eq!(r.status(), QueryStatus::LoadingWithData);
    assert!(r.is_data_stale(), "LoadingWithData with data => stale");

    // Complete with failure — data still stale.
    let rid2 = begin_request_id(&mut r, &mut s, 400, QueryFetchMode::Normal);
    r.complete_current_failure_with_data(rid2, "fallback", QueryError::response("err"), 500);
    assert_eq!(r.status(), QueryStatus::Failure);
    assert!(r.is_data_stale(), "Failure with data => stale");
}
