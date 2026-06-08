//! Tests for stale request rejection and two-phase completion protocol.

use crate::core::*;
use super::helpers::*;

// ── 6. Stale rejection for page fetches ─────────────────────────────────

#[test]
fn stale_request_success_is_rejected() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

    // Completing the first (stale) request should fail
    assert!(!r.complete_page_success(id1, vec!["stale".to_string()], true, true, 3_000));
    // Completing the second (current) request should succeed
    assert!(r.complete_page_success(id2, vec!["fresh".to_string()], false, true, 3_000));
    assert_eq!(r.page_count(), 1);
    assert_eq!(r.last_page(), Some(&vec!["fresh".to_string()]));
}

#[test]
fn stale_request_failure_is_rejected() {
    let mut r: InfiniteQueryResource<Vec<String>, String> = InfiniteQueryResource::new(
        QueryKey::from("items"),
        CachePolicy::NoCache,
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

    assert!(!r.complete_page_failure(id1, "stale error".to_string()));
    assert!(r.complete_page_failure(id2, "fresh error".to_string()));
    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.error(), Some(&"fresh error".to_string()));
}

#[test]
fn stale_request_increments_ignored_results() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

    assert!(!r.complete_page_success(id1, vec!["stale".to_string()], true, true, 3_000));
    assert_eq!(r.ignored_results(), 1);

    assert!(r.complete_page_success(id2, vec!["fresh".to_string()], false, true, 3_000));
    assert_eq!(r.ignored_results(), 1); // no increment for accepted result
}

#[test]
fn stale_failure_increments_ignored_results() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

    assert!(!r.complete_page_failure(id1, "stale".into()));
    assert_eq!(r.ignored_results(), 1);

    assert!(r.complete_page_failure(id2, "fresh".into()));
    assert_eq!(r.ignored_results(), 1);
}

// ── 13. Two-phase protocol ─────────────────────────────────────────────

#[test]
fn accept_current_request_returns_guard_for_active_request() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let guard = r.accept_current_request(id);
    assert!(guard.is_some());
    assert!(r.active_request_id().is_none()); // cleared on accept
}

#[test]
fn accept_current_request_rejects_stale_request() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let _id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

    let guard = r.accept_current_request(id1);
    assert!(guard.is_none());
}

#[test]
fn complete_success_with_guard_appends_page() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let guard = r.accept_current_request(id).unwrap();

    r.complete_success_with_guard(&guard, vec!["page1".to_string()], true, true, 2_000);
    assert_eq!(r.page_count(), 1);
    assert_eq!(r.status(), QueryStatus::Success);
    assert!(!r.is_fetching_next_page());
}

#[test]
fn complete_failure_with_guard_preserves_pages() {
    let mut r = load_n_pages(1);
    let mut seq = RequestSequencer::new();

    // load_n_pages sets has_next_page=false for the last page, re-enable
    r.set_has_next_page(true);

    let id = r.begin_fetch_next(&mut seq, 3_000).unwrap();
    let guard = r.accept_current_request(id).unwrap();
    r.complete_failure_with_guard(&guard, "network error".into());

    assert_eq!(r.status(), QueryStatus::Failure);
    assert_eq!(r.page_count(), 1);
    assert!(r.is_page_data_valid());
}
