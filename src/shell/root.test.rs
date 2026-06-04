use super::*;

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
