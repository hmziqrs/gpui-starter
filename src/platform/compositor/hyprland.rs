//! Hyprland compositor backend (IPC Unix socket).

#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::LOG;
use super::base::{CompositorCapabilities, get_display_title, is_app_window};
use super::{Compositor, WindowInfo};

/// Hyprland compositor client speaking the `~/.socket.sock` IPC protocol.
pub struct HyprlandCompositor {
    socket_path: PathBuf,
}

impl HyprlandCompositor {
    /// Create a new client.
    ///
    /// Returns `None` when `HYPRLAND_INSTANCE_SIGNATURE` is unset, so the
    /// detector can fall through to the next backend.
    pub fn new() -> Option<Self> {
        let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());

        let socket_path = PathBuf::from(format!("{runtime_dir}/hypr/{signature}/.socket.sock"));

        Some(Self { socket_path })
    }

    /// Send a single IPC command and read back the full textual response.
    fn send_command(&self, cmd: &str) -> Result<String> {
        let mut stream = UnixStream::connect(&self.socket_path).with_context(|| {
            format!(
                "failed to connect to Hyprland socket: {}",
                self.socket_path.display()
            )
        })?;

        stream
            .write_all(cmd.as_bytes())
            .context("failed to write command to Hyprland socket")?;

        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .context("failed to read response from Hyprland socket")?;

        Ok(response)
    }
}

impl Compositor for HyprlandCompositor {
    fn list_windows(&self) -> Result<Vec<WindowInfo>> {
        // `j/clients` returns a JSON array of client objects.
        let json = self.send_command("j/clients")?;
        let clients: Vec<HyprlandClient> =
            serde_json::from_str(&json).context("failed to parse Hyprland clients JSON")?;

        // `j/activewindow` tells us which address is focused.
        let active_address = self.active_window_address().unwrap_or_default();

        let windows = clients
            .into_iter()
            .filter(|c| {
                // Exclude unmapped or hidden windows.
                if !c.mapped || c.hidden {
                    return false;
                }
                // Exclude the host app itself.
                if is_app_window(&c.class, APP_ID) {
                    return false;
                }
                // Exclude windows with an empty class (special windows).
                if c.class.is_empty() {
                    return false;
                }
                true
            })
            .map(|c| WindowInfo {
                focused: active_address.as_deref() == Some(c.address.as_str()),
                workspace: c.workspace.id,
                address: c.address,
                title: get_display_title(&c.title, &c.class),
                class: c.class,
            })
            .collect();

        Ok(windows)
    }

    fn focus_window(&self, window_id: &str) -> Result<()> {
        let cmd = format!("dispatch focuswindow address:{window_id}");
        self.send_command(&cmd)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "Hyprland"
    }

    fn capabilities(&self) -> CompositorCapabilities {
        CompositorCapabilities::full()
    }
}

impl HyprlandCompositor {
    /// Query `j/activewindow` and return the focused client's address.
    fn active_window_address(&self) -> Result<Option<String>> {
        let json = self.send_command("j/activewindow")?;
        // The address field uses the `0x...` form; on failure degrade
        // gracefully by reporting no known focused window.
        let active: HyprlandActiveWindow =
            serde_json::from_str(&json).context("failed to parse Hyprland activewindow JSON")?;
        Ok(active.address)
    }
}

/// Default host application id used for self-filtering. Integration may
/// override this by passing an explicit `app_id` to the helpers below.
const APP_ID: &str = "gpui-starter";

/// Hyprland client (window) record from `j/clients`.
#[derive(Debug, Deserialize)]
struct HyprlandClient {
    address: String,
    title: String,
    class: String,
    workspace: HyprlandWorkspace,
    #[serde(default)]
    mapped: bool,
    #[serde(default)]
    hidden: bool,
}

/// Hyprland workspace record.
#[derive(Debug, Deserialize)]
struct HyprlandWorkspace {
    id: i32,
}

/// Subset of `j/activewindow` needed to resolve the focused address.
#[derive(Debug, Deserialize)]
struct HyprlandActiveWindow {
    address: Option<String>,
}

/// Apply blur layer rules for the host application on Hyprland.
///
/// Sets up transparency and blur effects via Hyprland IPC. The
/// `app_id` parameter is the layer-rule target (e.g. `"gpui-starter"`)
/// and is intentionally not hard-coded so the helper stays generic.
///
/// Returns `Ok(true)` if the rules were applied, or `Ok(false)` when not
/// running on Hyprland.
pub fn apply_blur_layer_rules(app_id: &str) -> Result<bool> {
    let Some(compositor) = HyprlandCompositor::new() else {
        return Ok(false);
    };

    let rules = [
        format!("blur,{app_id}"),
        format!("ignorezero,{app_id}"),
        format!("blurpopups,{app_id}"),
        format!("ignorealpha 0.35,{app_id}"),
    ];

    for rule in rules {
        let cmd = format!("keyword layerrule {rule}");
        compositor.send_command(&cmd).with_context(|| {
            format!("failed to apply Hyprland layer rule `{rule}` for `{app_id}`")
        })?;
    }

    tracing::info!(target: LOG, app_id, "applied Hyprland blur layer rules");
    Ok(true)
}
