//! Typed command/response layer that rides on gpui-starter's existing
//! newline-delimited JSON IPC framing (see [`super::IpcEndpoint`] and
//! `src/platform/process/single_instance.rs`).
//!
//! This module is intentionally pure data + serde: it defines the wire
//! types for a bidirectional request/response protocol and leaves the
//! actual socket plumbing to the integration stage. A second instance
//! (or any IPC client) encodes a [`ForwardedRequest`] as a single JSON
//! line; the primary instance decodes it, runs the [`ForwardedCommand`],
//! and replies with a [`ForwardedResponse`] line keyed by the shared
//! `id`.
//!
//! The framing already exists (one UTF-8 JSON object per `\n`). We do
//! **not** introduce tarpc, tokio-serde, or any length-prefixed codec.

use serde::{Deserialize, Serialize};

const LOG: &str = "gpui_starter::ipc::command";

/// A command that can be forwarded to the primary instance over IPC.
///
/// These mirror the high-level UI actions a second launch (or external
/// caller) may want to trigger in the already-running process. They are
/// kept generic and boilerplate-level: no launcher-specific modes,
/// themes, or item types leak through.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "payload")]
pub enum ForwardedCommand {
    /// Bring the main window to the foreground.
    ShowWindow,
    /// Hide the main window without quitting.
    HideWindow,
    /// Toggle the command palette (or equivalent quick-action UI).
    TogglePalette,
    /// Quit the running application cleanly.
    Quit,
    /// Reload configuration from disk.
    ReloadConfig,
    /// Open a deep link (`gpui-starter://...`) in the primary instance.
    DeepLink(String),
}

impl ForwardedCommand {
    /// A short, human-readable label suitable for tracing/log fields.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ShowWindow => "show_window",
            Self::HideWindow => "hide_window",
            Self::TogglePalette => "toggle_palette",
            Self::Quit => "quit",
            Self::ReloadConfig => "reload_config",
            Self::DeepLink(_) => "deep_link",
        }
    }
}

/// A single request envelope.
///
/// `id` correlates a [`ForwardedResponse`] back to its request across
/// the process boundary. Callers SHOULD mint monotonically increasing
/// ids; the only hard requirement is uniqueness among in-flight
/// requests (see [`super::rpc`] for the pending-request map).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForwardedRequest {
    pub id: u64,
    pub command: ForwardedCommand,
}

impl ForwardedRequest {
    /// Construct a new request with the given correlation id.
    pub fn new(id: u64, command: ForwardedCommand) -> Self {
        Self { id, command }
    }
}

/// The reply to a [`ForwardedRequest`], keyed by the same `id`.
///
/// `ok == true` with `error == None` denotes success. On failure `ok`
/// is `false` and `error` carries a best-effort, non-structured message
/// safe to surface in logs (never expose it verbatim to end users
/// without sanitizing).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForwardedResponse {
    pub id: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
}

impl ForwardedResponse {
    /// Build a success response for the given request id.
    pub fn ok(id: u64) -> Self {
        Self {
            id,
            ok: true,
            error: None,
        }
    }

    /// Build a failure response carrying `error`.
    pub fn error(id: u64, error: impl Into<String>) -> Self {
        let message = error.into();
        tracing::warn!(target: LOG, id, error = %message, "ipc request failed");
        Self {
            id,
            ok: false,
            error: Some(message),
        }
    }
}
