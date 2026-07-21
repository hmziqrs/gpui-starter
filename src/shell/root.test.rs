// Do NOT `use super::*` here — app_root/mod.rs does `use gpui::*`, whose glob brings
// gpui's `test` proc-macro into scope and makes bare `#[test]` resolve to `gpui::test`,
// which emits another `#[test]` and recurses infinitely (overflowing the stack under a
// high recursion_limit). Import the item under test explicitly instead.
use super::super::frame_time::is_slow_frame;

#[test]
fn is_slow_frame_below_threshold() {
    assert!(!is_slow_frame(3_999));
}

#[test]
fn is_slow_frame_at_threshold() {
    assert!(!is_slow_frame(4_000));
}

#[test]
fn is_slow_frame_above_threshold() {
    assert!(is_slow_frame(4_001));
}
