use std::{fmt, sync::Arc};

use gpui::{
    AnyWindowHandle, App, AppContext as _, BorrowAppContext as _, Global, SharedString, Window,
};
use gpui_component::WindowExt as _;

use super::backend::{NotificationBackend, NotifyRustBackend, UserNotifyBackend};
use super::inbox::{self, NotificationAttemptRecord, NotificationInboxKind};

const LOG: &str = "gpui_starter::notifications";
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

#[derive(Clone, Debug)]
pub struct NotificationRuntimeSnapshot {
    pub enabled_by_user: bool,
    pub permission: NotificationPermissionState,
    pub active_backend: NotificationBackendKind,
    pub capabilities: NotificationCapabilities,
    pub last_backend_error: Option<SharedString>,
    pub degraded_reason: Option<SharedString>,
}

impl NotificationRuntimeSnapshot {
    fn new(service: &NotificationService) -> Self {
        Self {
            enabled_by_user: true,
            permission: NotificationPermissionState::Unknown,
            active_backend: service.active_backend(),
            capabilities: service.active_capabilities(),
            last_backend_error: service.initial_error.clone().map(Into::into),
            degraded_reason: service.initial_error.clone().map(Into::into),
        }
    }
}

#[derive(Clone)]
pub struct NativeNotificationState {
    pub service: Arc<NotificationService>,
    pub snapshot: NotificationRuntimeSnapshot,
}

impl Global for NativeNotificationState {}

pub struct NotificationService {
    primary: Option<Arc<dyn NotificationBackend>>,
    secondary: Arc<dyn NotificationBackend>,
    initial_error: Option<String>,
}

impl NotificationService {
    pub fn new() -> Self {
        tracing::info!(target: LOG, "initializing native notification service");

        let mut initial_error = None;
        let primary = match UserNotifyBackend::new() {
            Ok(backend) => {
                tracing::info!(target: LOG, backend = %NotificationBackendKind::UserNotify, "primary notification backend selected");
                Some(Arc::new(backend) as Arc<dyn NotificationBackend>)
            }
            Err(err) => {
                tracing::warn!(
                    target: LOG,
                    backend = %NotificationBackendKind::UserNotify,
                    error = %err,
                    "primary notification backend unavailable; falling back"
                );
                initial_error = Some(err.to_string());
                None
            }
        };

        let service = Self {
            primary,
            secondary: Arc::new(NotifyRustBackend::new()),
            initial_error,
        };

        tracing::info!(
            target: LOG,
            active_backend = %service.active_backend(),
            capabilities = ?service.active_capabilities(),
            degraded_reason = ?service.initial_error,
            "native notification service initialized"
        );

        service
    }

    fn active_backend(&self) -> NotificationBackendKind {
        self.primary
            .as_ref()
            .map(|backend| backend.kind())
            .unwrap_or(NotificationBackendKind::NotifyRust)
    }

    fn active_capabilities(&self) -> NotificationCapabilities {
        self.primary
            .as_ref()
            .map(|backend| backend.capabilities())
            .unwrap_or_else(|| self.secondary.capabilities())
    }

    async fn refresh_permission_state(&self) -> NotificationPermissionState {
        tracing::debug!(
            target: LOG,
            active_backend = %self.active_backend(),
            "refreshing notification permission state"
        );

        if let Some(primary) = &self.primary {
            primary.refresh_permission_state().await
        } else if cfg!(target_os = "macos") {
            let state = NotificationPermissionState::Unavailable(
                self.initial_error
                    .clone()
                    .unwrap_or_else(|| "primary backend unavailable".to_string()),
            );
            tracing::info!(target: LOG, ?state, "permission state unavailable without primary backend");
            state
        } else {
            tracing::info!(target: LOG, "permission state unsupported on this platform");
            NotificationPermissionState::Unsupported
        }
    }

    async fn request_permission(&self) -> NotificationPermissionState {
        tracing::info!(
            target: LOG,
            active_backend = %self.active_backend(),
            "requesting notification permission"
        );

        if let Some(primary) = &self.primary {
            primary.request_permission().await
        } else if cfg!(target_os = "macos") {
            let state = NotificationPermissionState::Unavailable(
                self.initial_error
                    .clone()
                    .unwrap_or_else(|| "primary backend unavailable".to_string()),
            );
            tracing::warn!(target: LOG, ?state, "cannot request permission without primary backend");
            state
        } else {
            tracing::info!(target: LOG, "permission request unsupported on this platform");
            NotificationPermissionState::Unsupported
        }
    }

    pub async fn send(
        &self,
        request: NotificationRequest,
        enabled_by_user: bool,
    ) -> NotificationSendResult {
        tracing::info!(
            target: LOG,
            title = %request.title,
            importance = %request.importance,
            prefer_native = request.prefer_native,
            enabled_by_user,
            active_backend = %self.active_backend(),
            "native notification send requested"
        );

        if !enabled_by_user || !request.prefer_native {
            tracing::warn!(
                target: LOG,
                enabled_by_user,
                prefer_native = request.prefer_native,
                importance = %request.importance,
                "native send skipped; using in-app policy"
            );
            return NotificationSendResult {
                backend_used: NotificationBackendKind::UiOnly,
                degraded: request.importance == NotificationImportance::BackgroundWorthy,
                delivered_natively: false,
                error_summary: Some("native notifications are disabled".into()),
                importance: request.importance,
            };
        }

        let mut errors = Vec::new();

        if let Some(primary) = &self.primary {
            tracing::debug!(target: LOG, backend = %primary.kind(), "attempting primary notification send");
            match primary.send(&request).await {
                Ok(()) => {
                    tracing::info!(target: LOG, backend = %primary.kind(), "primary notification send succeeded");
                    return NotificationSendResult {
                        backend_used: primary.kind(),
                        degraded: false,
                        delivered_natively: true,
                        error_summary: None,
                        importance: request.importance,
                    };
                }
                Err(err) => {
                    tracing::warn!(
                        target: LOG,
                        backend = %primary.kind(),
                        error = %err,
                        "primary notification send failed"
                    );
                    errors.push(format!("{}: {err:#}", primary.kind()));
                }
            }
        }

        tracing::debug!(
            target: LOG,
            backend = %self.secondary.kind(),
            "attempting fallback notification send"
        );
        match self.secondary.send(&request).await {
            Ok(()) => {
                tracing::info!(
                    target: LOG,
                    backend = %self.secondary.kind(),
                    degraded = self.primary.is_some(),
                    "fallback notification send succeeded"
                );
                NotificationSendResult {
                    backend_used: self.secondary.kind(),
                    degraded: self.primary.is_some(),
                    delivered_natively: true,
                    error_summary: if errors.is_empty() {
                        None
                    } else {
                        Some(errors.join("; ").into())
                    },
                    importance: request.importance,
                }
            }
            Err(err) => {
                tracing::warn!(
                    target: LOG,
                    backend = %self.secondary.kind(),
                    error = %err,
                    "fallback notification send failed; using in-app policy"
                );
                errors.push(format!("{}: {err:#}", self.secondary.kind()));
                NotificationSendResult {
                    backend_used: NotificationBackendKind::UiOnly,
                    degraded: true,
                    delivered_natively: false,
                    error_summary: Some(errors.join("; ").into()),
                    importance: request.importance,
                }
            }
        }
    }
}

pub fn initialize(cx: &mut App) {
    let service = Arc::new(NotificationService::new());
    let snapshot = NotificationRuntimeSnapshot::new(&service);
    tracing::info!(
        target: LOG,
        active_backend = %snapshot.active_backend,
        permission = ?snapshot.permission,
        capabilities = ?snapshot.capabilities,
        degraded_reason = ?snapshot.degraded_reason,
        "installing native notification global state"
    );
    cx.set_global(NativeNotificationState { service, snapshot });
    let degraded = cx
        .global::<NativeNotificationState>()
        .snapshot
        .degraded_reason
        .is_some();
    crate::capabilities::set(
        "native_notifications",
        crate::capabilities::CapabilityStatus {
            supported: true,
            enabled: true,
            degraded,
            reason: cx
                .global::<NativeNotificationState>()
                .snapshot
                .degraded_reason
                .clone(),
            last_error: cx
                .global::<NativeNotificationState>()
                .snapshot
                .last_backend_error
                .clone(),
        },
        cx,
    );
    refresh_permission_state(cx);
}

pub fn snapshot(cx: &App) -> NotificationRuntimeSnapshot {
    cx.global::<NativeNotificationState>().snapshot.clone()
}

pub fn set_native_notifications_enabled(enabled: bool, cx: &mut App) {
    tracing::info!(target: LOG, enabled, "native notifications user setting changed");
    crate::app_state::update_config(cx, |config| {
        config.native_notifications_enabled = enabled;
    });
    mutate_snapshot(cx, |snapshot| {
        snapshot.enabled_by_user = enabled;
        if !enabled {
            snapshot.degraded_reason = Some("native notifications disabled by user".into());
        } else {
            snapshot.degraded_reason = None;
        }
    });
    let snapshot = cx.global::<NativeNotificationState>().snapshot.clone();
    crate::capabilities::set(
        "native_notifications",
        crate::capabilities::CapabilityStatus {
            supported: true,
            enabled,
            degraded: snapshot.degraded_reason.is_some(),
            reason: snapshot.degraded_reason,
            last_error: snapshot.last_backend_error,
        },
        cx,
    );
    inbox::record_attempt(
        NotificationAttemptRecord {
            title: "Notification Settings Updated".to_string(),
            body: if enabled {
                "Native notifications enabled".to_string()
            } else {
                "Native notifications disabled".to_string()
            },
            backend: NotificationBackendKind::UiOnly,
            delivered_natively: false,
            degraded: !enabled,
            error_summary: None,
            kind: NotificationInboxKind::SettingsUpdate,
        },
        cx,
    );
}

pub fn refresh_permission_state(cx: &mut App) {
    let service = cx.global::<NativeNotificationState>().service.clone();
    tracing::debug!(target: LOG, "scheduling async permission refresh");
    cx.spawn(async move |cx| {
        let permission = service.refresh_permission_state().await;
        tracing::info!(target: LOG, ?permission, "permission refresh completed");
        cx.update(move |cx| {
            mutate_snapshot(cx, |snapshot| {
                snapshot.permission = permission;
            });
        });
    })
    .detach();
}

pub fn request_permission_from_window(window: &mut Window, cx: &mut App) {
    let window_handle = window.window_handle();
    let service = cx.global::<NativeNotificationState>().service.clone();
    tracing::debug!(target: LOG, "scheduling async permission request");
    cx.spawn(async move |cx| {
        let permission = service.request_permission().await;
        tracing::info!(target: LOG, ?permission, "permission request completed");
        let message = format!("Notification permission: {}", permission.label());
        cx.update(move |cx| {
            mutate_snapshot(cx, |snapshot| {
                snapshot.permission = permission;
            });
            inbox::record_attempt(
                NotificationAttemptRecord {
                    title: "Notification Permission Updated".to_string(),
                    body: message.to_string(),
                    backend: NotificationBackendKind::UiOnly,
                    delivered_natively: false,
                    degraded: false,
                    error_summary: None,
                    kind: NotificationInboxKind::PermissionUpdate,
                },
                cx,
            );
            push_in_app_feedback(window_handle, message, cx);
        });
    })
    .detach();
}

pub fn send_from_window(request: NotificationRequest, window: &mut Window, cx: &mut App) {
    let window_handle = window.window_handle();
    let state = cx.global::<NativeNotificationState>();
    let service = state.service.clone();
    let enabled_by_user = state.snapshot.enabled_by_user
        && state.snapshot.permission != NotificationPermissionState::Denied;
    let fallback_message = request.body.clone();
    let inbox_title = request.title.to_string();
    let inbox_body = request.body.to_string();
    tracing::debug!(
        target: LOG,
        permission = ?state.snapshot.permission,
        enabled_by_user,
        "scheduling async notification send"
    );

    cx.spawn(async move |cx| {
        let result = service.send(request, enabled_by_user).await;
        tracing::info!(
            target: LOG,
            backend = %result.backend_used,
            degraded = result.degraded,
            delivered_natively = result.delivered_natively,
            error_summary = ?result.error_summary,
            "notification send completed"
        );
        let should_show_in_app = !result.delivered_natively
            && result.importance == NotificationImportance::ForegroundOnly;
        cx.update(move |cx| {
            apply_send_result(&result, cx);
            inbox::record_attempt(
                NotificationAttemptRecord {
                    title: inbox_title,
                    body: inbox_body,
                    backend: result.backend_used,
                    delivered_natively: result.delivered_natively,
                    degraded: result.degraded,
                    error_summary: result.error_summary.as_ref().map(ToString::to_string),
                    kind: NotificationInboxKind::Attempt,
                },
                cx,
            );
            if should_show_in_app {
                push_in_app_feedback(window_handle, fallback_message, cx);
            }
        });
    })
    .detach();
}

pub fn open_system_settings(cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        tracing::info!(target: LOG, "opening macOS notification settings");
        if let Err(err) = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.Notifications-Settings.extension")
            .spawn()
        {
            tracing::warn!(target: LOG, error = %err, "failed to open macOS notification settings");
            mutate_snapshot(cx, |snapshot| {
                snapshot.last_backend_error =
                    Some(format!("failed to open settings: {err}").into());
            });
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        tracing::warn!(target: LOG, "system notification settings unsupported on this platform");
        mutate_snapshot(cx, |snapshot| {
            snapshot.last_backend_error =
                Some("system notification settings are not supported on this platform".into());
        });
    }
}

pub fn apply_send_result_static(result: &NotificationSendResult, cx: &mut App) {
    apply_send_result(result, cx)
}

fn apply_send_result(result: &NotificationSendResult, cx: &mut App) {
    tracing::debug!(
        target: LOG,
        backend = %result.backend_used,
        degraded = result.degraded,
        delivered_natively = result.delivered_natively,
        error_summary = ?result.error_summary,
        "applying notification send result"
    );
    mutate_snapshot(cx, |snapshot| {
        snapshot.active_backend = result.backend_used;
        snapshot.last_backend_error = result.error_summary.clone();
        snapshot.degraded_reason = if result.degraded {
            result
                .error_summary
                .clone()
                .or_else(|| Some("notification delivery is degraded".into()))
        } else {
            None
        };
    });
}

fn mutate_snapshot(cx: &mut App, f: impl FnOnce(&mut NotificationRuntimeSnapshot)) {
    cx.update_global::<NativeNotificationState, _>(|state, cx| {
        f(&mut state.snapshot);
        cx.refresh_windows();
    });
}

fn push_in_app_feedback(
    window_handle: AnyWindowHandle,
    message: impl Into<SharedString>,
    cx: &mut App,
) {
    let message = message.into();
    tracing::debug!(target: LOG, message = %message, "showing in-app notification feedback");
    if let Err(err) = cx.update_window(window_handle, |_, window, cx| {
        window.push_notification(message, cx);
    }) {
        tracing::warn!(?err, "failed to show in-app notification fallback");
    }
}
