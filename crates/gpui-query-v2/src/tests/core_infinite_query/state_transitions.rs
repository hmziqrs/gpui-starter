//! Tests for page data access, reset, is_page_data_valid, failure preserving
//! pages, invalidate, and serde roundtrip.

use super::helpers::*;
use crate::core::*;

// ── 8. Page data access ────────────────────────────────────────────────

#[test]
fn pages_returns_vecdeque_in_order() {
    let r = load_n_pages(3);
    let pages = r.pages();

    assert_eq!(pages.len(), 3);
    assert_eq!(pages[0], vec!["page0".to_string()]);
    assert_eq!(pages[1], vec!["page1".to_string()]);
    assert_eq!(pages[2], vec!["page2".to_string()]);
}

#[test]
fn first_and_last_page_on_single_page() {
    let r = load_n_pages(1);
    assert_eq!(r.first_page(), Some(&vec!["page0".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["page0".to_string()]));
    assert!(r.has_data());
}

#[test]
fn first_and_last_page_none_when_empty() {
    let r = make_resource();
    assert!(r.first_page().is_none());
    assert!(r.last_page().is_none());
    assert!(!r.has_data());
}

// ── 10. Reset clears all pages ─────────────────────────────────────────

#[test]
fn reset_clears_all_pages_and_state() {
    let mut r = load_n_pages(3);
    assert!(r.has_data());
    assert_eq!(r.page_count(), 3);

    r.reset();

    assert!(r.pages().is_empty());
    assert_eq!(r.page_count(), 0);
    assert!(!r.has_data());
    assert_eq!(r.status(), QueryStatus::Idle);
    assert!(r.error().is_none());
    assert!(r.active_request_id().is_none());
    assert!(r.signal().is_none());
    assert!(r.started_at_ms().is_none());
    assert!(r.last_updated_at_ms().is_none());
    assert!(!r.is_fetching_next_page());
    assert!(!r.is_fetching_previous_page());

    // ForwardOnly defaults restored
    assert!(r.has_next_page());
    assert!(!r.has_previous_page());
}

#[test]
fn reset_preserves_max_pages() {
    let mut r = make_resource();
    r.set_max_pages(Some(10));
    r.reset();
    assert_eq!(r.max_pages(), Some(10));
}

#[test]
fn reset_preserves_direction() {
    let mut r = make_bidirectional_resource();
    r.reset();
    assert_eq!(r.direction(), FetchDirection::Bidirectional);
    assert!(!r.has_next_page());
    assert!(!r.has_previous_page());
}

#[test]
fn reset_clears_diagnostics() {
    let mut r = load_n_pages(2);
    r.increment_retry_count();
    r.increment_retry_count();

    r.reset();

    assert_eq!(r.retry_count(), 0);
    assert_eq!(r.ignored_results(), 0);
    assert_eq!(r.cancelled_count(), 0);
    assert_eq!(r.cache_hits(), 0);
}

// ── 12. is_page_data_valid across statuses ──────────────────────────────

#[test]
fn is_page_data_valid_false_when_idle() {
    let r = make_resource();
    assert!(!r.is_page_data_valid());
}

#[test]
fn is_page_data_valid_true_when_success_with_pages() {
    let r = load_n_pages(1);
    assert!(r.is_page_data_valid());
}

#[test]
fn is_page_data_valid_true_when_failure_with_existing_pages() {
    let mut r = load_n_pages(2);
    let mut seq = RequestSequencer::new();

    // load_n_pages sets has_next_page=false for the last page, re-enable
    r.set_has_next_page(true);

    let id = r.begin_fetch_next(&mut seq, 5_000).unwrap();
    r.complete_page_failure(id, "network error".into());

    // Failure does not clear existing pages
    assert_eq!(r.page_count(), 2);
    assert!(r.is_page_data_valid());
    assert_eq!(r.status(), QueryStatus::Failure);
}

#[test]
fn is_page_data_valid_false_when_failure_no_pages() {
    let mut r: InfiniteQueryResource<Vec<String>, String> = InfiniteQueryResource::new(
        QueryKey::from("items"),
        CachePolicy::NoCache,
        RequestPolicy::LatestWins,
    );
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_failure(id, "network error".to_string());

    assert!(!r.is_page_data_valid());
}

// ── 14. Failure does not clear existing pages ───────────────────────────

#[test]
fn page_failure_preserves_existing_pages() {
    let mut r = load_n_pages(2);
    let mut seq = RequestSequencer::new();

    assert_eq!(r.page_count(), 2);

    // load_n_pages sets has_next_page=false for the last page, re-enable
    r.set_has_next_page(true);

    let id = r.begin_fetch_next(&mut seq, 5_000).unwrap();
    r.complete_page_failure(id, "timeout".into());

    // Pages remain intact
    assert_eq!(r.page_count(), 2);
    assert_eq!(r.first_page(), Some(&vec!["page0".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["page1".to_string()]));
    assert_eq!(r.status(), QueryStatus::Failure);
}

// ── 17. Invalidate ─────────────────────────────────────────────────────

#[test]
fn invalidate_clears_last_updated_but_preserves_pages() {
    let mut r = load_n_pages(2);
    assert!(r.last_updated_at_ms().is_some());

    r.invalidate();

    assert!(r.last_updated_at_ms().is_none());
    assert_eq!(r.page_count(), 2);
}

// ── 18. Serde roundtrip ────────────────────────────────────────────────

#[test]
fn serde_roundtrip_preserves_state() {
    let r = load_n_pages(3);

    let json = serde_json::to_string(&r).unwrap();
    let back: InfiniteQueryResource<Vec<String>> = serde_json::from_str(&json).unwrap();

    assert_eq!(back.page_count(), 3);
    assert_eq!(back.status(), QueryStatus::Success);
    assert_eq!(back.first_page(), Some(&vec!["page0".to_string()]));
    assert_eq!(back.last_page(), Some(&vec!["page2".to_string()]));
    // Signal is skipped by serde
    assert!(back.signal().is_none());
}

#[test]
fn serde_wire_format_uses_plain_array() {
    let r = load_n_pages(2);
    let json = serde_json::to_string(&r).unwrap();
    // VecDeque serializes as a plain array, not a VecDeque-specific format
    assert!(json.contains("\"pages\":["));
    assert!(!json.contains("VecDeque"));
}
