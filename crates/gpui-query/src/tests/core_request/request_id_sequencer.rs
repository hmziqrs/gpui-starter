//! Tests for RequestId and RequestSequencer.
//!
//! Covers:
//! - RequestId: construction, fields, ordering, label, equality
//! - RequestSequencer: monotonicity, scope advancement, wrapping at u64::MAX

use crate::core::{RequestId, RequestSequencer};

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
