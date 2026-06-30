// Do NOT `use super::*` here. error_playground/mod.rs does `use gpui::{prelude::*, *}`,
// and gpui re-exports its `test` proc-macro at its crate root — so a glob import
// brings `gpui::test` into scope and a bare `#[test]` resolves to it. `gpui::test`
// rewrites the fn into another `#[test] fn …`, which resolves to `gpui::test` again,
// recursing infinitely (and, under a high recursion_limit, overflowing rustc's stack).
// Import only what we need so `#[test]` stays the builtin.
use super::ErrorPlaygroundPage;

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
