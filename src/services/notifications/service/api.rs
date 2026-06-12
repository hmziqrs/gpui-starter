use std::sync::Arc;

use gpui::{AnyWindowHandle, App, AppContext as _, BorrowAppContext as _, SharedString, Window};
use gpui_component::WindowExt as _;

use super::backend_service::{
    LOG, NativeNotificationState, NotificationRuntimeSnapshot, NotificationService,
};
use super::inbox::{self, NotificationAttemptRecord, NotificationInboxKind};
use super::types::{
    NotificationBackendKind, NotificationImportance, NotificationPermissionState,
    NotificationRequest, NotificationSendResult,
};

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
