use std::fmt;

use gpui::SharedString;

pub const CATEGORY_ACTIONS: &str = "gpui-starter.actions";
pub const CATEGORY_REPLY: &str = "gpui-starter.reply";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationBackendKind {
    UserNotify,
    NotifyRust,
    UiOnly,
}

impl fmt::Display for NotificationBackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UserNotify => f.write_str("user-notify"),
            Self::NotifyRust => f.write_str("notify-rust"),
            Self::UiOnly => f.write_str("in-app only"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotificationPermissionState {
    Unknown,
    Unsupported,
    Unavailable(String),
    NotDetermined,
    Denied,
    Authorized,
}

impl NotificationPermissionState {
    pub fn label(&self) -> SharedString {
        match self {
            Self::Unknown => "Unknown".into(),
            Self::Unsupported => "Unsupported on this platform".into(),
            Self::Unavailable(reason) => format!("Unavailable: {reason}").into(),
            Self::NotDetermined => "Not requested".into(),
            Self::Denied => "Denied".into(),
            Self::Authorized => "Authorized".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationImportance {
    ForegroundOnly,
    BackgroundWorthy,
}

impl fmt::Display for NotificationImportance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForegroundOnly => f.write_str("foreground-only"),
            Self::BackgroundWorthy => f.write_str("background-worthy"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct NotificationRequest {
    pub title: SharedString,
    pub body: SharedString,
    pub play_sound: bool,
    pub thread_id: Option<String>,
    pub category: Option<String>,
    pub prefer_native: bool,
    pub importance: NotificationImportance,
}

impl NotificationRequest {
    pub fn foreground(title: impl Into<SharedString>, body: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            play_sound: true,
            thread_id: None,
            category: None,
            prefer_native: true,
            importance: NotificationImportance::ForegroundOnly,
        }
    }

    pub fn action_buttons(title: impl Into<SharedString>, body: impl Into<SharedString>) -> Self {
        let mut request = Self::foreground(title, body);
        request.category = Some(CATEGORY_ACTIONS.to_string());
        request.thread_id = Some("settings-actions".to_string());
        request
    }

    pub fn reply(title: impl Into<SharedString>, body: impl Into<SharedString>) -> Self {
        let mut request = Self::foreground(title, body);
        request.category = Some(CATEGORY_REPLY.to_string());
        request.thread_id = Some("settings-reply".to_string());
        request
    }

    pub fn background_worthy(
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
    ) -> Self {
        let mut request = Self::foreground(title, body);
        request.importance = NotificationImportance::BackgroundWorthy;
        request.thread_id = Some("settings-background-worthy".to_string());
        request
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NotificationCapabilities {
    pub can_request_permission: bool,
    pub can_read_permission_state: bool,
    pub can_send_immediate_native: bool,
    pub can_send_interactive: bool,
    pub requires_packaged_runtime: bool,
}

#[derive(Clone, Debug)]
pub struct NotificationSendResult {
    pub backend_used: NotificationBackendKind,
    pub degraded: bool,
    pub delivered_natively: bool,
    pub error_summary: Option<SharedString>,
    pub importance: NotificationImportance,
}
