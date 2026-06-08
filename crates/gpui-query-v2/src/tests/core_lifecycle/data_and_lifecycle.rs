//! Invalidate, initial data seeding, placeholder data, cancelled count,
//! two-phase protocol, is_current_request, and full lifecycle tests (sections 23-29).

use crate::core::*;
use crate::tests::core_lifecycle::transitions::*;

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
