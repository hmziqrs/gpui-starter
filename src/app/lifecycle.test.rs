use super::*;

#[test]
fn lifecycle_stage_starts_as_starting() {
    let state = LifecycleState::default();
    assert_eq!(state.stage, LifecycleStage::Starting);
}

#[test]
fn take_render_panic_initially_false() {
    assert!(!take_render_panic());
}

#[test]
fn render_panic_flag_roundtrip() {
    // Set the flag manually (as the panic hook would).
    RENDER_PANIC_OCCURRED.store(true, Ordering::SeqCst);
    assert!(take_render_panic(), "first read should be true");
    assert!(!take_render_panic(), "second read should be false (flag reset)");
}

#[test]
fn enter_render_path_sets_and_clears() {
    assert!(!in_render_path());
    {
        let _guard = enter_render_path();
        assert!(in_render_path());
    }
    assert!(!in_render_path());
}

#[test]
fn track_recent_error_keeps_limit() {
    // Reset the slot by creating fresh state (test-only).
    for i in 0..25 {
        track_recent_error(format!("err-{i}"));
    }
    // The global slot should have kept only the last 20 entries.
    let slot = RECENT_ERRORS.get().unwrap();
    let guard = slot.lock().unwrap();
    assert_eq!(guard.len(), 20);
    assert_eq!(guard[0], "err-5");
    assert_eq!(guard[19], "err-24");
}

#[test]
fn check_previous_crash_none_when_no_marker() {
    // Ensure no stale marker exists.
    let path = std::env::temp_dir().join("gpui-starter.crash-marker");
    let _ = std::fs::remove_file(&path);
    assert!(check_previous_crash().is_none());
}

#[test]
fn write_and_check_crash_marker() {
    write_crash_marker();
    let contents = check_previous_crash();
    assert!(contents.is_some(), "marker should exist after write");
    let text = contents.unwrap();
    assert!(text.starts_with("pid="), "marker should start with pid=, got: {text}");
    // Clean up.
    remove_crash_marker();
    assert!(check_previous_crash().is_none(), "marker should be removed");
}
