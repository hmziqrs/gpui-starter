use std::fmt;

use gpui::SharedString;

pub const CATEGORY_ACTIONS: &str = "gpui-starter.actions";
pub const CATEGORY_REPLY: &str = "gpui-starter.reply";

// --- App identity (Linux desktop integration) --------------------------------
// Three DISTINCT ids — do not conflate (see the Linux notifications audit):
/// Display name shown verbatim by the daemon (notify-rust `.appname`).
pub const APP_DISPLAY_NAME: &str = "GPUI Starter";
/// Reverse-DNS desktop-entry id: the `.desktop` filename, notify-rust
/// `Hint::DesktopEntry`, and the Wayland app_id. (Also the macOS bundle id.)
pub const DESKTOP_ENTRY_ID: &str = "com.gpui-starter.app";
/// X11 WM_CLASS value (binary basename) — must equal `.desktop` StartupWMClass.
pub const WM_CLASS: &str = "gpui-starter";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationBackendKind {
    UserNotify,
    NotifyRust,
    #[cfg(feature = "notifications-portal")]
    Portal,
    UiOnly,
}

impl fmt::Display for NotificationBackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UserNotify => f.write_str("user-notify"),
            Self::NotifyRust => f.write_str("notify-rust"),
            #[cfg(feature = "notifications-portal")]
            Self::Portal => f.write_str("xdg-portal"),
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
    /// Force the in-app feedback toast to fire even when the native backend
    /// reports success. A successful D-Bus `Notify` only means the daemon
    /// *accepted* the notification — never that the user *saw* a banner (GNOME
    /// suppresses banners under Do-Not-Disturb / per-app mute / busy /
    /// fullscreen and still returns `Ok`). Set for explicit "send a test
    /// notification" affordances so the button can never look dead.
    pub force_in_app_feedback: bool,
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
            force_in_app_feedback: false,
        }
    }

    /// A user-initiated "send a test notification" affordance.
    ///
    /// Uses [`NotificationImportance::BackgroundWorthy`] so the native backend
    /// maps it to `Critical` urgency — the only urgency that pierces GNOME's
    /// Do-Not-Disturb, per-app banner mute, busy, and fullscreen gates — and
    /// forces in-app feedback, so the user always gets a visible response even
    /// when the banner is silently suppressed.
    pub fn test_notification(
        title: impl Into<SharedString>,
        body: impl Into<SharedString>,
    ) -> Self {
        let mut request = Self::foreground(title, body);
        request.importance = NotificationImportance::BackgroundWorthy;
        request.force_in_app_feedback = true;
        request
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
    /// True iff the native backend's send call returned `Ok` (e.g. the
    /// FreeDesktop `Notify` D-Bus call succeeded). **Not** proof the user saw
    /// a banner: GNOME/FreeDesktop returns `Ok` even when the banner is
    /// suppressed by Do-Not-Disturb, per-app mute, busy, or fullscreen, and the
    /// spec provides no display-confirmation signal. Treat as "accepted by the
    /// daemon", not "delivered to the user".
    pub delivered_natively: bool,
    pub error_summary: Option<SharedString>,
    pub importance: NotificationImportance,
}
