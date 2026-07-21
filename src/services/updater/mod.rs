mod apply;
mod check;
mod download;
mod types;

use gpui::UpdateGlobal as _;

// Re-export all public items so that external import paths remain unchanged.
pub use types::{CheckForUpdates, PlatformAsset, UpdateManifest, UpdateSnapshot, UpdateStatus};

// ---------------------------------------------------------------------------
// Initialize
// ---------------------------------------------------------------------------

pub fn initialize(cx: &mut gpui::App) {
    let channel = crate::app_state::update_channel(cx);
    cx.set_global(UpdateSnapshot {
        status: UpdateStatus::Idle,
        current_version: types::current_app_version(),
        last_check: None,
        update_channel: if channel.is_empty() {
            "stable".to_string()
        } else {
            channel
        },
        check_retry_count: 0,
        download_retry_count: 0,
        // Cache fields (cached_manifest / cached_asset) default to None via
        // `..Default::default()` — they are populated lazily by check_for_updates.
        ..Default::default()
    });

    // Register the CheckForUpdates action handler.
    cx.on_action(|_: &CheckForUpdates, cx| {
        check::check_for_updates(cx);
    });

    // Schedule a delayed startup check (5 seconds after launch).
    let startup_rt = cx
        .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
        .0
        .runtime
        .clone();
    cx.spawn(async move |cx| {
        startup_rt
            .spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(
                    types::STARTUP_CHECK_DELAY_SECS,
                ))
                .await;
            })
            .await
            .ok();
        cx.update(|cx| {
            tracing::info!(
                target: "gpui_starter::updater",
                "running startup update check"
            );
            check::check_for_updates(cx);
        });
    })
    .detach();

    // Schedule periodic re-check every 4 hours.
    let periodic_rt = cx
        .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
        .0
        .runtime
        .clone();
    cx.spawn(async move |cx| {
        loop {
            periodic_rt
                .spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(
                        types::PERIODIC_CHECK_INTERVAL_SECS,
                    ))
                    .await;
                })
                .await
                .ok();

            let should_check: bool = cx.update(|cx| {
                let snap = snapshot(cx);
                matches!(
                    snap.status,
                    UpdateStatus::Idle | UpdateStatus::UpToDate | UpdateStatus::Error(_)
                )
            });
            if should_check {
                cx.update(|cx| {
                    tracing::info!(
                        target: "gpui_starter::updater",
                        "running periodic update check"
                    );
                    check::check_for_updates(cx);
                });
            }
        }
    })
    .detach();

    tracing::info!(
        target: "gpui_starter::updater",
        version = %env!("CARGO_PKG_VERSION"),
        "updater service initialized"
    );
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

pub fn snapshot(cx: &gpui::App) -> UpdateSnapshot {
    cx.try_global::<UpdateSnapshot>()
        .cloned()
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Check for updates (re-export)
// ---------------------------------------------------------------------------

pub fn check_for_updates(cx: &mut gpui::App) {
    check::check_for_updates(cx);
}

// ---------------------------------------------------------------------------
// Download update (re-export)
// ---------------------------------------------------------------------------

pub fn download_update(cx: &mut gpui::App) {
    download::download_update(cx);
}

// ---------------------------------------------------------------------------
// Apply update (re-export)
// ---------------------------------------------------------------------------

pub fn apply_update(cx: &mut gpui::App) {
    apply::apply_update(cx);
}

// ---------------------------------------------------------------------------
// Check pending swap (re-export)
// ---------------------------------------------------------------------------

pub fn check_pending_swap(cx: &mut gpui::App) {
    apply::check_pending_swap(cx);
}

// ---------------------------------------------------------------------------
// Set channel
// ---------------------------------------------------------------------------

pub fn set_channel(channel: &str, cx: &mut gpui::App) {
    let ch = if channel.is_empty() {
        "stable".to_string()
    } else {
        channel.to_string()
    };
    UpdateSnapshot::update_global(cx, |snap, _cx| {
        snap.update_channel = ch.clone();
    });
    crate::app_state::update_config(cx, |config| {
        config.update_channel = ch;
    });
    tracing::info!(
        target: "gpui_starter::updater",
        channel = %channel,
        "update channel set"
    );
}

// ---------------------------------------------------------------------------
// Internal helpers — status management
// ---------------------------------------------------------------------------

fn set_status(status: UpdateStatus, cx: &mut gpui::App) {
    tracing::debug!(
        target: "gpui_starter::updater",
        status = ?status,
        "update status changed"
    );
    UpdateSnapshot::update_global(cx, |snap, _cx| {
        snap.status = status;
    });
}

fn reset_check_retry(cx: &mut gpui::App) {
    UpdateSnapshot::update_global(cx, |snap, _cx| {
        snap.check_retry_count = 0;
    });
}

fn reset_download_retry(cx: &mut gpui::App) {
    UpdateSnapshot::update_global(cx, |snap, _cx| {
        snap.download_retry_count = 0;
    });
}

// ---------------------------------------------------------------------------
// Internal helpers — notifications
// ---------------------------------------------------------------------------

/// Dispatch a native notification for "update available".
fn notify_update_available(version: &str, cx: &mut gpui::App) {
    dispatch_background_notification(
        &format!("Update v{version} available"),
        "A new version is available. Open settings to download and install.",
        cx,
    );
}

/// Dispatch a native notification for "update downloaded".
fn notify_update_downloaded(version: &str, cx: &mut gpui::App) {
    dispatch_background_notification(
        &format!("Update v{version} ready"),
        "Update downloaded — restart to apply.",
        cx,
    );
}

/// Dispatch a native notification for permanent update errors (retries exhausted).
fn notify_update_error(cx: &mut gpui::App) {
    dispatch_background_notification(
        "Update check failed",
        "Could not check or download updates. Please try again later.",
        cx,
    );
}

/// Send a notification via the notification service without requiring a window handle.
fn dispatch_background_notification(title: &str, body: &str, cx: &mut gpui::App) {
    // Record in the notification inbox.
    crate::notifications::inbox::record_attempt(
        crate::notifications::inbox::NotificationAttemptRecord {
            title: title.to_string(),
            body: body.to_string(),
            backend: crate::notifications::NotificationBackendKind::UiOnly,
            delivered_natively: false,
            degraded: false,
            error_summary: None,
            kind: crate::notifications::inbox::NotificationInboxKind::SettingsUpdate,
        },
        cx,
    );

    // Also attempt a native notification via the service if available.
    let state = match cx.try_global::<crate::notifications::NativeNotificationState>() {
        Some(s) => s.clone(),
        None => return,
    };
    let enabled_by_user = state.snapshot.enabled_by_user
        && state.snapshot.permission != crate::notifications::NotificationPermissionState::Denied;
    if !enabled_by_user {
        return;
    }

    let request = crate::notifications::NotificationRequest::background_worthy(title, body);
    let service = state.service.clone();

    cx.spawn(async move |cx| {
        let result = service.send(request, enabled_by_user).await;
        tracing::info!(
            target: "gpui_starter::updater",
            backend = %result.backend_used,
            delivered = result.delivered_natively,
            "update notification send completed"
        );
        cx.update(|cx| {
            crate::notifications::apply_send_result_static(&result, cx);
        });
    })
    .detach();
}

#[cfg(test)]
#[path = "../updater.test.rs"]
mod updater_test;
