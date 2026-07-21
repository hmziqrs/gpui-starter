use gpui::*;

use crate::{
    accessibility, app_state, capabilities, commands, connectivity, crash_report, desktop_actions,
    error_surface,
    lifecycle::{LifecycleStage, LifecycleState},
    logging, notifications, secure_storage, session, shortcuts, storage, telemetry, undo_stack,
};

use super::row;

pub fn build_diagnostic_rows(cx: &App) -> Vec<Div> {
    // Borrow AppState instead of cloning the whole struct (it now carries
    // notification_inbox + two permission HashSets + AppConfig). It is only
    // read below, so a shared borrow is sufficient and lives for the function.
    let state = cx.try_global::<app_state::AppState>();
    let lifecycle = cx
        .try_global::<LifecycleState>()
        .cloned()
        .unwrap_or_else(LifecycleState::starting);
    let notifications = notifications::snapshot(cx);
    let connectivity = connectivity::snapshot(cx);
    let secure_storage = secure_storage::snapshot(cx);
    let session = session::snapshot(cx);
    let logging = logging::snapshot(cx);
    let storage = storage::snapshot(cx);
    let telemetry = telemetry::snapshot(cx);
    let accessibility = accessibility::snapshot(cx);
    let capabilities = capabilities::snapshot(cx);
    let shortcuts = shortcuts::snapshot(cx);
    let desktop_actions = desktop_actions::snapshot(cx);
    let undo = undo_stack::snapshot(cx);
    let latest_error = error_surface::latest(cx);
    let crash_snap = crash_report::snapshot(cx);

    let command_registry = commands::registry();
    let mut command_titles = Vec::with_capacity(command_registry.len());
    let mut command_states = Vec::with_capacity(command_registry.len());
    for command in &command_registry {
        command_titles.push(command.title.to_string());
        let availability = commands::availability(command.id, cx);
        let reason = availability
            .disabled_reason
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string());
        command_states.push(format!(
            "{}: enabled={} reason={}",
            command.title, availability.enabled, reason
        ));
    }
    let command_titles = command_titles.join(", ");
    let command_states = command_states.join(" | ");

    let (undo_last_label, undo_last_timestamp) = undo
        .past
        .last()
        .map(|entry| (entry.label.clone(), entry.created_at.to_rfc3339()))
        .unwrap_or_else(|| ("None".to_string(), "None".to_string()));

    let lifecycle_label = match lifecycle.stage {
        LifecycleStage::Starting => "Starting",
        LifecycleStage::Running => "Running",
        LifecycleStage::ShuttingDown => "ShuttingDown",
        LifecycleStage::Crashed => "Crashed",
    };
    let lifecycle_panic_summary =
        crate::lifecycle::last_panic_summary().unwrap_or_else(|| "None".to_string());

    let mut rows = vec![
        row("App", env!("CARGO_PKG_NAME")),
        row("Version", env!("CARGO_PKG_VERSION")),
        row("Lifecycle", lifecycle_label),
        row(
            "Lifecycle Startup Step",
            lifecycle.startup_step.as_deref().unwrap_or("None"),
        ),
        row(
            "Lifecycle Shutdown Step",
            lifecycle.shutdown_step.as_deref().unwrap_or("None"),
        ),
        row(
            "Lifecycle Startup Error",
            lifecycle.last_startup_error.as_deref().unwrap_or("None"),
        ),
        row(
            "Lifecycle Shutdown Error",
            lifecycle.last_shutdown_error.as_deref().unwrap_or("None"),
        ),
        row("Lifecycle Panic Summary", &lifecycle_panic_summary),
        row(
            "Notification Backend",
            &notifications.active_backend.to_string(),
        ),
        row(
            "Notification Permission",
            notifications.permission.label().as_ref(),
        ),
        row(
            "Notification Degraded",
            notifications.degraded_reason.as_deref().unwrap_or("No"),
        ),
        row("Connectivity", &format!("{:?}", connectivity.state)),
        row("Connectivity Probe URL", &connectivity.probe_url),
        row(
            "Connectivity Last Error",
            connectivity.last_error.as_deref().unwrap_or("None"),
        ),
        row(
            "Secure Storage Available",
            if secure_storage.available {
                "Yes"
            } else {
                "No"
            },
        ),
        row(
            "Secure Storage Error",
            secure_storage.last_error.as_deref().unwrap_or("None"),
        ),
        row("Session", &format!("{:?}", session.state)),
        row("Commands", &command_registry.len().to_string()),
        row("Command Titles", &command_titles),
        row("Command Availability", &command_states),
        row(
            "First Run Pending",
            if crate::first_run::is_pending(cx) {
                "Yes"
            } else {
                "No"
            },
        ),
        row(
            "Logging Enabled",
            if logging.enabled { "Yes" } else { "No" },
        ),
        row(
            "Logging Guard Active",
            if logging.has_guard { "Yes" } else { "No" },
        ),
        row(
            "Logging Error",
            logging.last_error.as_deref().unwrap_or("None"),
        ),
        row(
            "Storage Available",
            if storage.available { "Yes" } else { "No" },
        ),
        row(
            "Storage Healthy",
            if storage.healthy { "Yes" } else { "No" },
        ),
        row("Storage DB Path", &storage.db_path),
        row(
            "Storage Schema Version",
            &storage.schema_version.to_string(),
        ),
        row(
            "Storage Last Maintenance",
            storage.last_maintenance_at.as_deref().unwrap_or("None"),
        ),
        row(
            "Storage Last Migration",
            storage.last_migration_result.as_deref().unwrap_or("None"),
        ),
        row(
            "Storage Error",
            storage.last_error.as_deref().unwrap_or("None"),
        ),
        row(
            "Telemetry Compiled",
            if telemetry.compiled { "Yes" } else { "No" },
        ),
        row(
            "Telemetry Consented",
            if telemetry.consented { "Yes" } else { "No" },
        ),
        row(
            "Telemetry Enabled",
            if telemetry.enabled { "Yes" } else { "No" },
        ),
        row("Telemetry Mode", &format!("{:?}", telemetry.mode)),
        row(
            "Telemetry Endpoint",
            telemetry.endpoint_redacted.as_deref().unwrap_or("None"),
        ),
        row(
            "Telemetry Error",
            telemetry.last_error.as_deref().unwrap_or("None"),
        ),
        row(
            "Telemetry Export Error",
            telemetry.last_export_error.as_deref().unwrap_or("None"),
        ),
        row(
            "Telemetry Events Recorded",
            &telemetry.events_recorded.to_string(),
        ),
        row(
            "Accessibility AccessKit Linked",
            if accessibility.accesskit_linked {
                "Yes"
            } else {
                "No"
            },
        ),
        row(
            "Accessibility Bridge Enabled",
            if accessibility.bridge_enabled {
                "Yes"
            } else {
                "No"
            },
        ),
        row("Accessibility Status", &accessibility.status),
        row(
            "Desktop Clipboard Available",
            if desktop_actions.clipboard_available {
                "Yes"
            } else {
                "No"
            },
        ),
        row(
            "Desktop Picker Available",
            if desktop_actions.picker_available {
                "Yes"
            } else {
                "No"
            },
        ),
        row(
            "Desktop Opener Available",
            if desktop_actions.opener_available {
                "Yes"
            } else {
                "No"
            },
        ),
        row(
            "Desktop Active Watchers",
            &desktop_actions.active_watchers.to_string(),
        ),
        row(
            "Desktop Last Error",
            desktop_actions.last_error.as_deref().unwrap_or("None"),
        ),
        row("Undo Stack Size", &undo.past.len().to_string()),
        row("Redo Stack Size", &undo.future.len().to_string()),
        row("Undo Last Label", &undo_last_label),
        row("Undo Last Timestamp", &undo_last_timestamp),
        row(
            "Undo Last Rejected",
            undo.last_rejected.as_deref().unwrap_or("None"),
        ),
        row(
            "Shortcut Enabled (Config)",
            if shortcuts.enabled { "Yes" } else { "No" },
        ),
        row(
            "Shortcut Registered",
            if shortcuts.registered { "Yes" } else { "No" },
        ),
        row("Shortcut Accelerator", &shortcuts.accelerator),
        row(
            "Shortcut Error",
            shortcuts.last_error.as_deref().unwrap_or("None"),
        ),
        row(
            "Error Surface Count",
            &error_surface::record_count(cx).to_string(),
        ),
        row(
            "Latest Error",
            latest_error
                .as_ref()
                .map(|error| error.message.as_str())
                .unwrap_or("None"),
        ),
        row(
            "Crash Reports Pending",
            &crash_snap.pending_count.to_string(),
        ),
        row(
            "Crash Reports Last Timestamp",
            crash_snap.last_crash_timestamp.as_deref().unwrap_or("None"),
        ),
        row(
            "Crash Reports Upload Endpoint",
            if crash_snap.upload_endpoint.is_empty() {
                "None"
            } else {
                &crash_snap.upload_endpoint
            },
        ),
        row(
            "Crash Reports Last Upload Error",
            crash_snap.last_upload_error.as_deref().unwrap_or("None"),
        ),
    ];

    if let Some(app_state) = state {
        let fallback_log_dir = app_state.paths.log_dir.display().to_string();
        let display_log_dir = if logging.log_dir.is_empty() {
            fallback_log_dir.as_str()
        } else {
            logging.log_dir.as_str()
        };
        rows.push(row(
            "Config Dir",
            &app_state.paths.config_dir.display().to_string(),
        ));
        rows.push(row(
            "Data Dir",
            &app_state.paths.data_dir.display().to_string(),
        ));
        rows.push(row(
            "Cache Dir",
            &app_state.paths.cache_dir.display().to_string(),
        ));
        rows.push(row("Log Dir", display_log_dir));
        rows.push(row("Log File Prefix", &logging.file_prefix));
        rows.push(row(
            "State File",
            &app_state.paths.state_file.display().to_string(),
        ));
        rows.push(row(
            "Active Route",
            &app_state.config.active_route.to_url().to_string(),
        ));
        rows.push(row("Config Version", &app_state.config.version.to_string()));
        rows.push(row(
            "State Load Error",
            app_state.last_load_error.as_deref().unwrap_or("None"),
        ));
        rows.push(row(
            "State Save Error",
            app_state.last_save_error.as_deref().unwrap_or("None"),
        ));
    }

    for (name, status) in capabilities {
        let value = format!(
            "supported={} enabled={} degraded={} reason={} error={}",
            status.supported,
            status.enabled,
            status.degraded,
            status.reason.as_deref().unwrap_or("-"),
            status.last_error.as_deref().unwrap_or("-")
        );
        rows.push(row(&format!("Capability:{name}"), &value));
    }

    rows
}
