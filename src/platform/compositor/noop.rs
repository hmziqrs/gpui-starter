//! No-op compositor implementation for unsupported environments.

#![cfg(target_os = "linux")]

use super::base::CompositorCapabilities;
use super::{Compositor, WindowInfo};

/// A no-op compositor that reports no windows and no capabilities.
///
/// Used as a safe, always-available fallback when no supported
/// compositor is detected, so the module compiles and ships with zero
/// required call sites. Every method degrades gracefully without error.
#[allow(dead_code)]
pub struct NoopCompositor;

impl Compositor for NoopCompositor {
    fn list_windows(&self) -> anyhow::Result<Vec<WindowInfo>> {
        Ok(Vec::new())
    }

    fn focus_window(&self, _window_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn name(&self) -> &'static str {
        "Noop"
    }

    fn capabilities(&self) -> CompositorCapabilities {
        CompositorCapabilities::none()
    }
}
