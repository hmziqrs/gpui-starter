use async_trait::async_trait;

use super::NotificationBackend;
use crate::notifications::{
    APP_DISPLAY_NAME, DESKTOP_ENTRY_ID, NotificationBackendKind, NotificationCapabilities,
    NotificationPermissionState, NotificationRequest,
};

const LOG: &str = "gpui_starter::notifications::notify_rust";

pub struct NotifyRustBackend;

impl NotifyRustBackend {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl NotificationBackend for NotifyRustBackend {
    fn kind(&self) -> NotificationBackendKind {
        NotificationBackendKind::NotifyRust
    }

    fn capabilities(&self) -> NotificationCapabilities {
        NotificationCapabilities {
            can_request_permission: false,
            can_read_permission_state: false,
            can_send_immediate_native: true,
            can_send_interactive: false,
            requires_packaged_runtime: cfg!(target_os = "windows"),
        }
    }

    async fn refresh_permission_state(&self) -> NotificationPermissionState {
        NotificationPermissionState::Unsupported
    }

    async fn request_permission(&self) -> NotificationPermissionState {
        NotificationPermissionState::Unsupported
    }

    async fn send(&self, request: &NotificationRequest) -> anyhow::Result<()> {
        tracing::info!(
            target: LOG,
            title = %request.title,
            importance = %request.importance,
            "sending notification through notify-rust"
        );

        let mut notification = notify_rust::Notification::new();
        notification
            .appname(APP_DISPLAY_NAME)
            .summary(&request.title)
            .body(&request.body);

        // Hint::DesktopEntry is Linux/D-Bus-only (notify-rust's macOS backend
        // doesn't have it). It tells the daemon which desktop-entry we are so it
        // can resolve our .desktop + icon (.appname alone is freeform, never
        // looked up).
        #[cfg(target_os = "linux")]
        notification.hint(notify_rust::Hint::DesktopEntry(
            DESKTOP_ENTRY_ID.to_string(),
        ));

        // Urgency: GNOME defaults an omitted urgency to Normal, which
        // Do-Not-Disturb / per-app banner mute / busy / fullscreen suppress. Only
        // Critical pierces all those gates (GNOME Shell js/ui/messageTray.js), so
        // background-worthy events (including the test affordance) map to Critical;
        // routine foreground events stay Normal. Linux/D-Bus only — notify-rust's
        // macOS backend has no urgency concept.
        #[cfg(target_os = "linux")]
        notification.urgency(match request.importance {
            crate::notifications::NotificationImportance::BackgroundWorthy => {
                notify_rust::Urgency::Critical
            }
            crate::notifications::NotificationImportance::ForegroundOnly => {
                notify_rust::Urgency::Normal
            }
        });

        if request.play_sound {
            notification.sound_name("default");
        }

        #[cfg(target_os = "linux")]
        {
            // show_async() is a real async future driven by zbus's own executor
            // — no blocking, safe to await here. The sync show() would block_on
            // (async-io) and park the executor that polls this async trait.
            match notification.show_async().await {
                Ok(_) => {
                    tracing::info!(target: LOG, "notify-rust send succeeded");
                    Ok(())
                }
                Err(err) => {
                    tracing::warn!(target: LOG, error = %err, "notify-rust send failed");
                    Err(err.into())
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            // macOS/Windows: only the sync show() exists (native backend — no
            // zbus block_on, so it doesn't park the executor like on Linux).
            match notification.show() {
                Ok(_) => {
                    tracing::info!(target: LOG, "notify-rust send succeeded");
                    Ok(())
                }
                Err(err) => {
                    tracing::warn!(target: LOG, error = %err, "notify-rust send failed");
                    Err(err.into())
                }
            }
        }
    }
}
