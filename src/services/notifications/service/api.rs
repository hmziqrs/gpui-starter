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
    refresh_daemon_state(cx);
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

/// Probe the FreeDesktop notification daemon once at startup and annotate the
/// snapshot so Settings can show "no daemon" immediately — instead of only
/// revealing it after the first failed send.
///
/// `notify_rust::get_server_information` / `get_capabilities` are synchronous
/// wrappers over `zbus::block_on` (async-io) that PARK the calling thread, so
/// they run on the tokio runtime's blocking pool — never on the gpui main/UI
/// thread. The send path is unaffected: NotifyRust stays the active backend and
/// still returns a real D-Bus `Err` on no-daemon (firing the in-app fallback).
pub fn refresh_daemon_state(cx: &mut App) {
    // Linux-only: notify-rust's get_server_information / get_capabilities (and
    // the whole FreeDesktop D-Bus daemon concept) are not compiled on macOS,
    // which uses UNUserNotificationCenter. On other platforms this is a no-op.
    #[cfg(target_os = "linux")]
    {
        let Some(runtime) = crate::services::tokio_runtime::handle(cx) else {
            tracing::debug!(target: LOG, "no tokio runtime available; skipping daemon probe");
            return;
        };
        tracing::debug!(target: LOG, "scheduling async notification-daemon probe");
        cx.spawn(async move |cx| {
            let probe = runtime
                .spawn_blocking(|| {
                    // Both calls hit org.freedesktop.Notifications over D-Bus.
                    let info = notify_rust::get_server_information();
                    let caps = notify_rust::get_capabilities();
                    (info, caps)
                })
                .await;
            let (info, caps) = match probe {
                Ok((info, caps)) => (info.ok(), caps.ok()),
                Err(err) => {
                    tracing::warn!(target: LOG, error = %err, "daemon probe task failed");
                    (None, None)
                }
            };
            let daemon_present = info.is_some();
            let caps_display = caps.as_ref().map(|c| c.join(", ")).map(SharedString::from);
            tracing::info!(
                target: LOG,
                daemon_present,
                server = ?info.as_ref().map(|i| i.name.clone()),
                capabilities = ?caps,
                "notification-daemon probe completed"
            );
            cx.update(move |cx| {
                mutate_snapshot(cx, |snapshot| {
                    snapshot.daemon_capabilities = caps_display;
                    if daemon_present {
                        snapshot.degraded_reason = None;
                        snapshot.last_backend_error = None;
                    } else {
                        snapshot.degraded_reason = Some(
                            "No notification daemon owns org.freedesktop.Notifications — \
                             install notify-osd / dunst / mako"
                                .into(),
                        );
                        snapshot.last_backend_error = Some(
                            "org.freedesktop.Notifications is not reachable on the session bus"
                                .into(),
                        );
                    }
                });
            });
        })
        .detach();
    }

    #[cfg(not(target_os = "linux"))]
    {
        // No D-Bus notification daemon on macOS/Windows — nothing to probe.
        let _ = cx;
        tracing::debug!(target: LOG, "notification-daemon probe is Linux-only; skipped");
    }
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
    // Capture before `request` is moved into service.send().
    let force_in_app_feedback = request.force_in_app_feedback;
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
        // A successful native send is NOT proof a banner was shown — DND /
        // per-app mute / busy / fullscreen all suppress while the daemon still
        // returns Ok — so explicit test affordances force in-app feedback
        // regardless of `delivered_natively`.
        let should_show_in_app = force_in_app_feedback
            || (!result.delivered_natively
                && result.importance == NotificationImportance::ForegroundOnly);
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
                let feedback: SharedString = if force_in_app_feedback && result.delivered_natively {
                    // The daemon accepted the call but display is not
                    // guaranteed — point the user at the two common
                    // silent-suppression causes instead of feigning success.
                    format!(
                        "Sent to the {} notification daemon. If no banner appeared, \
                         check Do Not Disturb and the per-app setting for \"{}\".",
                        result.backend_used,
                        crate::notifications::DESKTOP_ENTRY_ID,
                    )
                    .into()
                } else {
                    // Native delivery failed for a known reason (e.g. "no
                    // daemon owns org.freedesktop.Notifications") — surface
                    // that instead of silently re-showing the body.
                    result
                        .error_summary
                        .as_ref()
                        .map(|err| format!("Native notification unavailable: {err}").into())
                        .unwrap_or(fallback_message)
                };
                push_in_app_feedback(window_handle, feedback, cx);
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

    #[cfg(target_os = "linux")]
    {
        // GNOME/Ubuntu: gnome-control-center is standard. Fall back to xdg-open
        // on the settings URI for other desktops. Best-effort — a missing
        // binary just logs a warning (the test-feedback message already gives
        // the user the manual path).
        tracing::info!(target: LOG, "opening Linux notification settings");
        if std::process::Command::new("gnome-control-center")
            .arg("notifications")
            .spawn()
            .is_ok()
        {
            return;
        }
        tracing::debug!(target: LOG, "gnome-control-center unavailable; trying xdg-open");
        if let Err(err) = std::process::Command::new("xdg-open")
            .arg("settings://notifications")
            .spawn()
        {
            tracing::warn!(target: LOG, error = %err, "failed to open Linux notification settings");
            mutate_snapshot(cx, |snapshot| {
                snapshot.last_backend_error =
                    Some("could not open notification settings; open them manually".into());
            });
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
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
