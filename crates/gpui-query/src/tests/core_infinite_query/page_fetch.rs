//! Tests for page append (fetch_next_page), prepend (fetch_previous_page),
//! signal cancellation on fetch replacement, loading status transitions,
//! and has_more propagation.

use crate::core::*;
use super::helpers::*;

// ── 2. Page append (fetch_next_page) ────────────────────────────────────

#[test]
fn fetch_next_page_appends_page_and_updates_status() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    assert_eq!(r.status(), QueryStatus::LoadingEmpty);
    assert!(r.is_fetching_next_page());
    assert!(r.signal().is_some());

    let accepted = r.complete_page_success(
        id,
        vec!["a".to_string(), "b".to_string()],
        true,
        true,
        2_000,
    );
    assert!(accepted);
    assert_eq!(r.page_count(), 1);
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.last_page(), Some(&vec!["a".to_string(), "b".to_string()]));
    assert_eq!(r.last_updated_at_ms(), Some(2_000));
    assert!(!r.is_fetching_next_page());
    assert!(r.signal().is_none());
}

#[test]
fn fetch_next_page_accumulates_multiple_pages() {
    let r = load_n_pages(5);

    assert_eq!(r.page_count(), 5);
    assert_eq!(r.first_page(), Some(&vec!["page0".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["page4".to_string()]));
    // The last page was loaded with has_more = false
    assert!(!r.has_next_page());
}

#[test]
fn fetch_next_page_returns_none_when_no_next_page() {
    let mut r = make_resource();
    r.set_has_next_page(false);
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000);
    assert!(id.is_none());
}

// ── 3. Page prepend (fetch_previous_page) ───────────────────────────────

#[test]
fn fetch_previous_page_prepends_page() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    // Load initial page via next
    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id1, vec!["page1".to_string()], true, true, 2_000);

    // Enable previous and prepend
    r.set_has_previous_page(true);
    assert!(!r.is_fetching_previous_page()); // not fetching yet
    let id2 = r.begin_fetch_previous(&mut seq, 3_000).unwrap();
    assert!(r.is_fetching_previous_page());

    let accepted = r.complete_page_success(
        id2,
        vec!["page0".to_string()],
        false,
        false, // is_next = false => prepend
        4_000,
    );
    assert!(accepted);
    assert_eq!(r.page_count(), 2);
    assert_eq!(r.first_page(), Some(&vec!["page0".to_string()]));
    assert_eq!(r.last_page(), Some(&vec!["page1".to_string()]));
    // has_more from completion updated has_previous_page
    assert!(!r.has_previous_page());
}

#[test]
fn fetch_previous_page_returns_none_when_no_previous_page() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();
    // ForwardOnly: has_previous_page defaults to false
    let id = r.begin_fetch_previous(&mut seq, 1_000);
    assert!(id.is_none());
}

// ── 7. Signal cancellation on page fetch replacement ────────────────────

#[test]
fn begin_fetch_next_cancels_previous_signal() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let _id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let old_signal = r.signal().unwrap().clone();
    assert!(!old_signal.is_cancelled());

    // Starting a new fetch cancels the old signal
    let _id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();
    assert!(old_signal.is_cancelled());
    // The new signal is not cancelled
    assert!(!r.signal().unwrap().is_cancelled());
    assert_ne!(r.signal().unwrap(), &old_signal);
}

#[test]
fn begin_fetch_previous_cancels_previous_signal() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let _id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let old_signal = r.signal().unwrap().clone();
    assert!(!old_signal.is_cancelled());

    // Switching direction cancels the old signal
    r.set_has_previous_page(true);
    let _id2 = r.begin_fetch_previous(&mut seq, 2_000).unwrap();
    assert!(old_signal.is_cancelled());
    assert!(!r.is_fetching_next_page());
    assert!(r.is_fetching_previous_page());
}

#[test]
fn latest_wins_allows_replacement() {
    let mut r = make_resource(); // LatestWins
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let id2 = r.begin_fetch_next(&mut seq, 2_000).unwrap();

    assert_ne!(id1, id2);
    assert_eq!(r.cancelled_count(), 1);
}

#[test]
fn ignore_while_loading_prevents_replacement() {
    let mut r = InfiniteQueryResource::<Vec<String>>::new(
        QueryKey::from("items"),
        CachePolicy::Ttl { ttl_ms: 60_000 },
        RequestPolicy::IgnoreWhileLoading,
    );
    let mut seq = RequestSequencer::new();

    let _id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    let id2 = r.begin_fetch_next(&mut seq, 2_000);
    assert!(id2.is_none());
    assert_eq!(r.cancelled_count(), 0);
}

// ── 15. Loading status transitions ──────────────────────────────────────

#[test]
fn loading_empty_when_no_pages_exist() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let _id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    assert_eq!(r.status(), QueryStatus::LoadingEmpty);
    assert!(r.is_loading());
}

#[test]
fn loading_with_data_when_pages_exist() {
    let mut r = load_n_pages(1);
    let mut seq = RequestSequencer::new();

    // load_n_pages sets has_next_page=false for the last page, re-enable
    r.set_has_next_page(true);

    let _id = r.begin_fetch_next(&mut seq, 5_000).unwrap();
    assert_eq!(r.status(), QueryStatus::LoadingWithData);
    assert!(r.is_loading());
}

// ── 16. has_more propagation ────────────────────────────────────────────

#[test]
fn has_more_false_stops_further_fetches() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id, vec!["only".to_string()], false, true, 2_000);

    assert!(!r.has_next_page());
    // Attempting to fetch next should return None
    let id2 = r.begin_fetch_next(&mut seq, 3_000);
    assert!(id2.is_none());
}

#[test]
fn has_more_propagated_on_prepend() {
    let mut r = make_resource();
    let mut seq = RequestSequencer::new();

    let id1 = r.begin_fetch_next(&mut seq, 1_000).unwrap();
    r.complete_page_success(id1, vec!["page1".to_string()], true, true, 2_000);

    r.set_has_previous_page(true);
    let id2 = r.begin_fetch_previous(&mut seq, 3_000).unwrap();
    // has_more = false => has_previous_page set to false
    r.complete_page_success(id2, vec!["page0".to_string()], false, false, 4_000);

    assert!(!r.has_previous_page());
}
