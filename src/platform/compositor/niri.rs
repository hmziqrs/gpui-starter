//! Niri compositor backend (newline-delimited JSON IPC socket).

#![cfg(target_os = "linux")]

use std::io::{BufRead, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use super::base::{CompositorCapabilities, get_display_title, is_app_window};
use super::{Compositor, WindowInfo};

/// Niri compositor client speaking the `NIRI_SOCKET` IPC protocol.
pub struct NiriCompositor {
    socket_path: PathBuf,
}

impl NiriCompositor {
    /// Create a new client.
    ///
    /// Returns `None` when `NIRI_SOCKET` is unset.
    pub fn new() -> Option<Self> {
        Some(Self {
            socket_path: std::env::var("NIRI_SOCKET").ok()?.into(),
        })
    }

    /// Send a single IPC request and read back the first (newline-
    /// delimited) JSON response line.
    fn send_command(&self, cmd: &str) -> Result<String> {
        let mut stream = UnixStream::connect(&self.socket_path).with_context(|| {
            format!(
                "failed to connect to Niri socket: {}",
                self.socket_path.display()
            )
        })?;

        stream
            .write_all(cmd.as_bytes())
            .context("failed to write command to Niri socket")?;

        let reader = std::io::BufReader::new(stream);
        let response = reader
            .lines()
            .next()
            .ok_or_else(|| anyhow!("Niri socket closed without a response"))?
            .context("failed to read response from Niri socket")?;

        Ok(response)
    }
}

impl Compositor for NiriCompositor {
    fn name(&self) -> &'static str {
        "Niri"
    }

    fn focus_window(&self, window_id: &str) -> Result<()> {
        // Niri expects one JSON action per line.
        let cmd = format!(r#"{{"Action":{{"FocusWindow":{{"id":{window_id}}}}}}}\n"#);
        self.send_command(&cmd)?;
        Ok(())
    }

    fn list_windows(&self) -> Result<Vec<WindowInfo>> {
        let json_string = self.send_command("\"Windows\"\n")?;

        // Niri may reply with an error object instead of a `Windows` payload.
        let niri_result: std::result::Result<NiriWindowReply, serde_json::Value> =
            serde_json::from_str(&json_string).context("failed to parse Niri windows JSON")?;

        let niri_reply = niri_result
            .map_err(|err| anyhow!("Niri returned an error to Windows request: {err}"))?;

        let mut window_info = Vec::new();
        for window in niri_reply.windows {
            if is_app_window(&window.app_id, APP_ID) {
                continue;
            }

            window_info.push(WindowInfo {
                address: format!("{}", window.id),
                title: get_display_title(&window.title, &window.app_id),
                class: window.app_id,
                workspace: window.workspace_id as i32,
                focused: window.is_focused,
            });
        }

        Ok(window_info)
    }

    fn capabilities(&self) -> CompositorCapabilities {
        CompositorCapabilities::full()
    }
}

/// Default host application id used for self-filtering.
const APP_ID: &str = "gpui-starter";

#[derive(Debug, Deserialize)]
struct NiriWindowReply {
    #[serde(rename = "Windows")]
    windows: Vec<NiriWindow>,
}

#[derive(Debug, Deserialize)]
struct NiriWindow {
    id: i64,
    title: String,
    app_id: String,
    workspace_id: i64,
    is_focused: bool,
}
