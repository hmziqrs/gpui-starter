//! Compositor abstraction for Wayland/X11 window management.
//!
//! This module provides a trait-based abstraction for interacting with
//! Linux compositors to enumerate windows and switch focus. Backends are
//! provided for Hyprland and Niri (both via their IPC sockets). A
//! [`NoopCompositor`] fallback ships so the module is safe to compile in
//! even when nothing detects it.
//!
//! Detection runs an environment-variable cascade in [`detect_compositor`]:
//! `HYPRLAND_INSTANCE_SIGNATURE` -> `KDE_SESSION_VERSION` -> `NIRI_SOCKET`
//! -> `None`. KWin/MangoWM are intentionally out of scope here.
//!
//! All types are Linux-only: the entire module is gated behind
//! `#[cfg(target_os = "linux")]` and is excluded from the macOS-primary
//! build.

#![cfg(target_os = "linux")]

pub mod base;
mod detect;
pub mod hyprland;
pub mod niri;
mod noop;

pub use base::CompositorCapabilities;
pub use detect::detect_compositor;

use std::fmt;

/// Tracing target idiom shared by every file in this module.
pub(crate) const LOG: &str = "gpui_starter::compositor";

/// Information about an open window reported by the compositor.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    /// Unique compositor-specific window identifier (e.g. `"0x5678abcd"`
    /// for Hyprland, or a numeric id for Niri).
    pub address: String,
    /// Window title, falling back to the class when empty (see
    /// [`base::get_display_title`]).
    pub title: String,
    /// Application class / app-id (e.g. `"firefox"`, `"org.kde.dolphin"`).
    pub class: String,
    /// Workspace number the window lives on (`-1` when unknown).
    pub workspace: i32,
    /// Whether this window is currently focused.
    pub focused: bool,
}

/// Trait for compositor window-management operations.
///
/// Implementations must be thread-safe (`Send + Sync`) as the compositor
/// may be queried from background executors.
pub trait Compositor: Send + Sync {
    /// List all open "normal" user windows.
    ///
    /// Layer-shell windows (panels, bars), the host application itself
    /// and other special windows should be filtered out by the backend.
    fn list_windows(&self) -> anyhow::Result<Vec<WindowInfo>>;

    /// Focus/activate a window by its compositor-specific address.
    fn focus_window(&self, window_id: &str) -> anyhow::Result<()>;

    /// Compositor name, for logging and diagnostics.
    fn name(&self) -> &'static str;

    /// Capabilities advertised by this backend. The default reports no
    /// capabilities (matching [`noop::NoopCompositor`]).
    fn capabilities(&self) -> CompositorCapabilities {
        CompositorCapabilities::none()
    }
}

impl fmt::Debug for dyn Compositor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Compositor({})", self.name())
    }
}
