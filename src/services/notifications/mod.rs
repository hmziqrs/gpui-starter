mod backend;
pub mod inbox;
mod service;

pub use service::{
    CATEGORY_ACTIONS, CATEGORY_REPLY, NativeNotificationState, NotificationPermissionState,
    NotificationRequest, NotificationRuntimeSnapshot, apply_send_result_static, initialize,
    open_system_settings, request_permission_from_window, send_from_window,
    set_native_notifications_enabled, snapshot,
};

pub(crate) use service::{NotificationBackendKind, NotificationCapabilities};
