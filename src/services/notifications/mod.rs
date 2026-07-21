mod backend;
pub mod inbox;
mod service;

pub use service::{
    APP_DISPLAY_NAME, CATEGORY_ACTIONS, CATEGORY_REPLY, DESKTOP_ENTRY_ID, NativeNotificationState,
    NotificationPermissionState, NotificationRequest, NotificationRuntimeSnapshot, WM_CLASS,
    apply_send_result_static, initialize, open_system_settings, request_permission_from_window,
    send_from_window, set_native_notifications_enabled, snapshot,
};

pub(crate) use service::{NotificationBackendKind, NotificationCapabilities};
// NotificationImportance is only consumed via `crate::notifications::` inside
// Linux-gated notify-rust code (urgency mapping) — re-exporting it
// unconditionally leaves the import unused on macOS/Windows.
#[cfg(target_os = "linux")]
pub(crate) use service::NotificationImportance;
