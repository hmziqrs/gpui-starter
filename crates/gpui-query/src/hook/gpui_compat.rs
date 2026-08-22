//! Compatibility shim for reading GPUI entities across divergent `read_with`
//! signatures.
//!
//! `Entity::read_with` returns `R` directly in older gpui (e.g. the Zed git
//! revisions some apps pin), but returns `C::Result<R>` — which is
//! `Result<R>` for `AsyncApp` — in gpui 0.2.2 (crates.io). A single call site
//! cannot name both return types, so [`read_entity`] makes the inner closure
//! return `()`. `read_with` therefore yields `()` on the older API and
//! `Result<()>` on 0.2.2; both are discarded, and the real value is captured
//! through a mutable local. The captured value is identical on either version,
//! so the crate compiles unchanged against both.

/// Read a value from a GPUI entity in a way that is source-compatible with both
/// the `R`-returning and the `Result<R>`-returning `Entity::read_with`.
///
/// Returns `Some(R)` on success, or `None` if the entity could not be read
/// (e.g. dropped or accessed off-thread) under the `Result`-returning API.
pub(crate) fn read_entity<T: 'static, R, C: gpui::AppContext>(
    entity: &gpui::Entity<T>,
    cx: &C,
    f: impl FnOnce(&T, &gpui::App) -> R,
) -> Option<R> {
    let mut out: Option<R> = None;
    let _ = entity.read_with(cx, |value, app| {
        out = Some(f(value, app));
    });
    out
}
