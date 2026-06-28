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
            .body(&request.body)
            // Tell the daemon which desktop-entry we are so it can resolve our
            // .desktop file + icon (.appname is freeform and never looked up).
            .hint(notify_rust::Hint::DesktopEntry(
                DESKTOP_ENTRY_ID.to_string(),
            ));

        if request.play_sound {
            notification.sound_name("default");
        }

        // show_async() is a real async future driven by zbus's own executor — no
        // blocking, safe to await here. The sync show() would block_on and park
        // the executor thread that polls this async trait.
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
}
