//! Compositor detection via an environment-variable cascade.

#![cfg(target_os = "linux")]

use super::Compositor;
use super::LOG;
use super::hyprland::HyprlandCompositor;
use super::niri::NiriCompositor;

/// Detect the running compositor and return a boxed client for it.
///
/// Detection cascade (first hit wins):
/// 1. **Hyprland** &mdash; `HYPRLAND_INSTANCE_SIGNATURE` is set.
/// 2. **KWin** &mdash; `KDE_SESSION_VERSION` is set (currently unimplemented;
///    reserved for a future feature, so it yields `None` here).
/// 3. **Niri** &mdash; `NIRI_SOCKET` is set.
/// 4. **None** &mdash; no supported compositor detected.
///
/// Returns `Some(Box<dyn Compositor>)` for a detected backend, or `None`
/// when nothing matched. Callers that want a always-available handle can
/// fall back to [`super::noop::NoopCompositor`] explicitly.
pub fn detect_compositor() -> Option<Box<dyn Compositor>> {
    // 1. Hyprland
    if let Some(compositor) = HyprlandCompositor::new() {
        tracing::info!(target: LOG, "detected Hyprland compositor");
        return Some(Box::new(compositor));
    }

    // 2. KWin (reserved for a future `kwin` feature; not implemented here).
    if std::env::var_os("KDE_SESSION_VERSION").is_some() {
        tracing::debug!(
            target: LOG,
            "KDE_SESSION_VERSION set but KWin backend is not enabled; skipping"
        );
    }

    // 3. Niri
    if let Some(compositor) = NiriCompositor::new() {
        tracing::info!(target: LOG, "detected Niri compositor");
        return Some(Box::new(compositor));
    }

    // 4. Nothing detected.
    tracing::warn!(target: LOG, "no supported compositor detected; window switching disabled");
    None
}
