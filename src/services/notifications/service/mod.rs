mod api;
mod backend_service;
mod types;

pub use api::{
    apply_send_result_static, initialize, open_system_settings, request_permission_from_window,
    send_from_window, set_native_notifications_enabled, snapshot,
};
pub use backend_service::{NativeNotificationState, NotificationRuntimeSnapshot};
pub use types::{
    APP_DISPLAY_NAME, CATEGORY_ACTIONS, CATEGORY_REPLY, DESKTOP_ENTRY_ID, NotificationBackendKind,
    NotificationCapabilities, NotificationPermissionState, NotificationRequest, WM_CLASS,
};
// NotificationImportance is consumed (via this service re-export) only by the
// Linux-only urgency mapping; gate it so the macOS/Windows lane has no unused import.
#[cfg(target_os = "linux")]
pub use types::NotificationImportance;

// Re-export items from parent modules that our sub-modules need internally.
// This keeps the super:: references clean.
#[cfg(feature = "notifications-portal")]
pub(super) use crate::services::notifications::backend::PortalBackend;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) use crate::services::notifications::backend::UserNotifyBackend;
pub(super) use crate::services::notifications::backend::{NotificationBackend, NotifyRustBackend};
pub(super) use crate::services::notifications::inbox;
