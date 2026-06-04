use super::*;

// ---------------------------------------------------------------------------
// ErrorPlaygroundPage::new() default state
// ---------------------------------------------------------------------------

#[test]
fn new_has_all_results_none() {
    let page = ErrorPlaygroundPage::new();
    assert!(page.http_result.is_none());
    assert!(page.fs_result.is_none());
    assert!(page.async_result.is_none());
    assert!(page.background_panic_result.is_none());
}

// ---------------------------------------------------------------------------
// Default trait
// ---------------------------------------------------------------------------

#[test]
fn default_matches_new() {
    let from_new = ErrorPlaygroundPage::new();
    let from_default = ErrorPlaygroundPage::default();
    assert!(from_default.http_result.is_none());
    assert!(from_default.fs_result.is_none());
    assert!(from_default.async_result.is_none());
    assert!(from_default.background_panic_result.is_none());
    // Both produce identical state.
    assert!(from_new.http_result == from_default.http_result);
    assert!(from_new.fs_result == from_default.fs_result);
    assert!(from_new.async_result == from_default.async_result);
    assert!(from_new.background_panic_result == from_default.background_panic_result);
}
