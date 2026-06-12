//! Tests for initial state, empty pages state, and FetchDirection modes.

use super::helpers::*;
use crate::core::*;

// ── 1. Initial state ────────────────────────────────────────────────────

#[test]
fn new_resource_has_idle_state_with_empty_pages() {
    let r = make_resource();

    assert_eq!(r.status(), QueryStatus::Idle);
    assert!(r.pages().is_empty());
    assert_eq!(r.page_count(), 0);
    assert!(!r.has_data());
    assert!(r.first_page().is_none());
    assert!(r.last_page().is_none());
    assert!(r.error().is_none());
    assert!(!r.is_loading());
    assert!(r.signal().is_none());
    assert!(r.active_request_id().is_none());
    assert!(r.started_at_ms().is_none());
    assert!(r.last_updated_at_ms().is_none());

    // v2: ForwardOnly defaults
    assert!(r.has_next_page());
    assert!(!r.has_previous_page());
    assert!(!r.is_fetching_next_page());
    assert!(!r.is_fetching_previous_page());

    // v2: bounded default
    assert_eq!(r.max_pages(), Some(50));
    assert_eq!(r.direction(), FetchDirection::ForwardOnly);

    // Diagnostics
    assert_eq!(r.cache_hits(), 0);
    assert_eq!(r.cancelled_count(), 0);
    assert_eq!(r.ignored_results(), 0);
    assert_eq!(r.retry_count(), 0);

    // Empty pages means data is not valid
    assert!(!r.is_page_data_valid());
}

// ── 9. Empty pages state ───────────────────────────────────────────────

#[test]
fn empty_pages_state_is_idle() {
    let r = make_resource();
    assert!(r.pages().is_empty());
    assert_eq!(r.page_count(), 0);
    assert!(!r.has_data());
    assert!(!r.is_page_data_valid());
    assert_eq!(r.status(), QueryStatus::Idle);
}

// ── 11. FetchDirection modes ────────────────────────────────────────────

#[test]
fn forward_only_defaults_has_next_true() {
    let r = make_resource();
    assert_eq!(r.direction(), FetchDirection::ForwardOnly);
    assert!(r.has_next_page());
    assert!(!r.has_previous_page());
}

#[test]
fn bidirectional_defaults_both_false() {
    let r = make_bidirectional_resource();
    assert_eq!(r.direction(), FetchDirection::Bidirectional);
    assert!(!r.has_next_page());
    assert!(!r.has_previous_page());
}

#[test]
fn bidirectional_rejects_fetch_next_without_opt_in() {
    let mut r = make_bidirectional_resource();
    let mut seq = RequestSequencer::new();
    assert!(r.begin_fetch_next(&mut seq, 1_000).is_none());
}

#[test]
fn bidirectional_allows_fetch_after_opt_in() {
    let mut r = make_bidirectional_resource();
    let mut seq = RequestSequencer::new();
    r.set_has_next_page(true);
    assert!(r.begin_fetch_next(&mut seq, 1_000).is_some());
}

#[test]
fn set_direction_changes_reset_behavior() {
    let mut r = make_resource();
    assert!(r.has_next_page()); // ForwardOnly default

    r.set_direction(FetchDirection::Bidirectional);
    r.reset();
    assert!(!r.has_next_page()); // Bidirectional reset default
    assert!(!r.has_previous_page());
}
