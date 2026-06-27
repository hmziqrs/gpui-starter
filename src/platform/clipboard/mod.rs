//! Clipboard utilities for gpui-starter derived apps.
//!
//! Re-exports the portable content type ([`ClipboardContent`]), an in-memory
//! [`ClipboardHistory`] store, and fallible arboard write wrappers
//! ([`set_text`] / [`set_image`] / [`set_content`]) unconditionally.
//!
//! The optional Wayland change monitor is only compiled when BOTH
//! `target_os = "linux"` and the `clipboard-history` cargo feature are
//! active; on every other configuration (including the default no-feature
//! build) it is absent, so the default build pulls in only `arboard` + std.

pub mod copy;
pub mod data;
pub mod item;

#[cfg(all(target_os = "linux", feature = "clipboard-history"))]
pub mod monitor;

pub use copy::{ClipboardError, set_content, set_image, set_text};
pub use data::{ClipboardHistory, DEFAULT_CAPACITY};
pub use item::{ClipboardContent, ClipboardItem};

#[cfg(all(target_os = "linux", feature = "clipboard-history"))]
pub use monitor::{MonitorHandle, start_monitor};
