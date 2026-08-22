//! Data retention tests: placeholder_data, previous_data, display_data, rollback.

use crate::core::*;
use crate::tests::core_cache::*;
use crate::tests::test_support::*;

// ══════════════════════════════════════════════════════════════════════════
// DATA RETENTION: placeholder_data, previous_data, display_data, rollback
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn placeholder_data_set_and_clear() {
    let mut r = ttl_resource();
    assert_eq!(r.placeholder_data(), None);
    r.set_placeholder_data(Some("loading..."));
    assert_eq!(r.placeholder_data(), Some(&"loading..."));
    r.set_placeholder_data(None);
    assert_eq!(r.placeholder_data(), None);
}

#[test]
fn display_data_prefers_real_data_over_placeholder() {
    let mut r = ttl_resource();
    r.set_placeholder_data(Some("placeholder"));
    seed_data(&mut r, "real", STORED_AT_MS);
    assert_eq!(r.display_data(), Some(&"real"));
}

#[test]
fn display_data_falls_back_to_placeholder_when_no_data() {
    let mut r = ttl_resource();
    r.set_placeholder_data(Some("placeholder"));
    assert_eq!(r.data(), None);
    assert_eq!(r.display_data(), Some(&"placeholder"));
}

#[test]
fn display_data_none_when_neither_set() {
    let r = ttl_resource();
    assert_eq!(r.display_data(), None);
}

#[test]
fn previous_data_tracked_across_successive_successes() {
    let mut r = ttl_resource();
    seed_data(&mut r, "first", 100);
    assert_eq!(r.previous_data(), None, "no previous on first success");
    seed_data(&mut r, "second", 200);
    assert_eq!(r.data(), Some(&"second"));
    assert_eq!(r.previous_data(), Some(&"first"));
}

#[test]
fn previous_data_preserved_across_failure() {
    let mut r = ttl_resource();
    seed_data(&mut r, "v1", 100);
    seed_data(&mut r, "v2", 200);
    r.apply_failure("error", 300);
    assert_eq!(r.data(), Some(&"v2"), "failure preserves current data");
    assert_eq!(r.previous_data(), Some(&"v1"), "failure does not touch previous_data");
}

#[test]
fn rollback_restores_previous_data() {
    let mut r = ttl_resource();
    seed_data(&mut r, "original", 100);
    seed_data(&mut r, "updated", 200);
    let rolled_back = r.rollback_to_previous();
    assert!(rolled_back);
    assert_eq!(r.data(), Some(&"original"));
    assert_eq!(r.status(), QueryStatus::Success);
    assert_eq!(r.previous_data(), None, "previous_data cleared after rollback");
}

#[test]
fn rollback_returns_false_when_no_previous() {
    let mut r = ttl_resource();
    seed_data(&mut r, "only", 100);
    assert!(!r.rollback_to_previous());
    assert_eq!(r.data(), Some(&"only"));
}

#[test]
fn set_data_optimistic_update_saves_previous() {
    let mut r = ttl_resource();
    seed_data(&mut r, "original", 100);
    r.set_data("optimistic");
    assert_eq!(r.data(), Some(&"optimistic"));
    assert_eq!(r.previous_data(), Some(&"original"));
}

#[test]
fn clear_data_saves_to_previous() {
    let mut r = ttl_resource();
    seed_data(&mut r, "existing", 100);
    r.clear_data();
    assert_eq!(r.data(), None);
    assert_eq!(r.previous_data(), Some(&"existing"));
}

#[test]
fn rollback_after_optimistic_update() {
    let mut r = ttl_resource();
    seed_data(&mut r, "original", 100);
    r.set_data("optimistic");
    let rolled_back = r.rollback_to_previous();
    assert!(rolled_back);
    assert_eq!(r.data(), Some(&"original"));
    assert_eq!(r.status(), QueryStatus::Success);
}

#[test]
fn reset_clears_placeholder_and_previous() {
    let mut r = ttl_resource();
    seed_data(&mut r, "first", 100);
    seed_data(&mut r, "second", 200);
    r.set_placeholder_data(Some("placeholder"));
    assert_eq!(r.previous_data(), Some(&"first"));
    assert_eq!(r.placeholder_data(), Some(&"placeholder"));
    r.reset();
    assert_eq!(r.placeholder_data(), None);
    assert_eq!(r.previous_data(), None);
    assert_eq!(r.data(), None);
}
