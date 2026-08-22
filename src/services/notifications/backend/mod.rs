mod notify_rust;
#[cfg(feature = "notifications-portal")]
mod portal;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod user_notify;

pub use notify_rust::NotifyRustBackend;
#[cfg(feature = "notifications-portal")]
pub use portal::PortalBackend;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use user_notify::UserNotifyBackend;

use async_trait::async_trait;

use crate::notifications::{
    NotificationBackendKind, NotificationCapabilities, NotificationPermissionState,
    NotificationRequest,
};

#[async_trait]
pub trait NotificationBackend: Send + Sync {
    fn kind(&self) -> NotificationBackendKind;
    fn capabilities(&self) -> NotificationCapabilities;
    async fn refresh_permission_state(&self) -> NotificationPermissionState;
    async fn request_permission(&self) -> NotificationPermissionState;
    async fn send(&self, request: &NotificationRequest) -> anyhow::Result<()>;
}
