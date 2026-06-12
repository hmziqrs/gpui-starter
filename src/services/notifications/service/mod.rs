mod api;
mod backend_service;
mod types;

pub use api::{
    apply_send_result_static, initialize, open_system_settings, request_permission_from_window,
    send_from_window, set_native_notifications_enabled, snapshot,
};
pub use backend_service::{NativeNotificationState, NotificationRuntimeSnapshot};
pub use types::{
    CATEGORY_ACTIONS, CATEGORY_REPLY, NotificationBackendKind, NotificationCapabilities,
    NotificationPermissionState, NotificationRequest,
};

// Re-export items from parent modules that our sub-modules need internally.
// This keeps the super:: references clean.
pub(super) use crate::services::notifications::backend::{
    NotificationBackend, NotifyRustBackend, UserNotifyBackend,
};
pub(super) use crate::services::notifications::inbox;
