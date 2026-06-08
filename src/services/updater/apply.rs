use std::path::PathBuf;

use super::types::*;
use gpui::App;

// ---------------------------------------------------------------------------
// Apply update
// ---------------------------------------------------------------------------

pub fn apply_update(cx: &mut App) {
    let current = super::snapshot(cx);
    let (version, path) = match &current.status {
        UpdateStatus::Downloaded { version, path } => (version.clone(), path.clone()),
        _ => {
            tracing::warn!(
                target: "gpui_starter::updater",
                status = ?current.status,
                "apply requested but no downloaded update"
            );
            return;
        }
    };

    tracing::info!(
        target: "gpui_starter::updater",
        version = %version,
        path = %path,
        "scheduling update swap on next launch"
    );

    super::set_status(UpdateStatus::ReadyToInstall, cx);

    // Write a marker file so the app launcher can perform the swap on next boot.
    let marker_path = pending_swap_path();
    let pending = serde_json::json!({
        "version": version,
        "source_path": path,
        "scheduled_at": chrono::Utc::now().to_rfc3339(),
    });
    if let Err(err) = std::fs::write(&marker_path, pending.to_string()) {
        tracing::error!(
            target: "gpui_starter::updater",
            path = %marker_path.display(),
            error = %err,
            "failed to write pending swap marker"
        );
        super::set_status(
            UpdateStatus::Error(format!("failed to schedule swap: {err}")),
            cx,
        );
    }
}

// ---------------------------------------------------------------------------
// Check and apply pending swap on startup
// ---------------------------------------------------------------------------

pub fn check_pending_swap(cx: &mut App) {
    let marker_path = pending_swap_path();
    if !marker_path.exists() {
        return;
    }

    tracing::info!(
        target: "gpui_starter::updater",
        path = %marker_path.display(),
        "pending swap marker found, attempting binary swap"
    );

    let data = match std::fs::read_to_string(&marker_path) {
        Ok(d) => d,
        Err(err) => {
            tracing::error!(
                target: "gpui_starter::updater",
                error = %err,
                "failed to read pending swap marker"
            );
            return;
        }
    };

    let pending: serde_json::Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(err) => {
            tracing::error!(
                target: "gpui_starter::updater",
                error = %err,
                "failed to parse pending swap marker"
            );
            // Remove corrupt marker so we don't retry indefinitely.
            let _ = std::fs::remove_file(&marker_path);
            return;
        }
    };

    let source_path = match pending.get("source_path").and_then(|v| v.as_str()) {
        Some(p) => PathBuf::from(p),
        None => {
            tracing::error!(
                target: "gpui_starter::updater",
                "pending swap marker missing source_path"
            );
            let _ = std::fs::remove_file(&marker_path);
            return;
        }
    };

    let version = pending
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    if !source_path.exists() {
        tracing::error!(
            target: "gpui_starter::updater",
            path = %source_path.display(),
            "pending swap source path does not exist"
        );
        let _ = std::fs::remove_file(&marker_path);
        return;
    }

    let current_exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(err) => {
            tracing::error!(
                target: "gpui_starter::updater",
                error = %err,
                "failed to determine current executable path"
            );
            return;
        }
    };

    // Detect whether we are inside a .app bundle on macOS.
    let swap_result: Result<(), String> = (|| {
        #[cfg(target_os = "macos")]
        {
            // If the current exe is inside a .app bundle, swap the entire bundle.
            if let Some(bundle_path) = current_exe
                .ancestors()
                .find(|a| a.extension().is_some_and(|ext| ext == "app"))
            {
                // The downloaded source might also be a .app bundle or a directory
                // that should replace the bundle.
                let source_bundle = if source_path.extension().is_some_and(|ext| ext == "app") {
                    source_path.clone()
                } else if source_path.is_dir() {
                    source_path.clone()
                } else {
                    // Standalone binary inside a bundle — replace the binary directly.
                    let dest_binary = current_exe.clone();
                    std::process::Command::new("mv")
                        .arg("-f")
                        .arg(&source_path)
                        .arg(&dest_binary)
                        .status()
                        .map_err(|e| format!("failed to mv binary: {e}"))?;
                    return Ok(());
                };

                tracing::info!(
                    target: "gpui_starter::updater",
                    source = %source_bundle.display(),
                    dest = %bundle_path.display(),
                    "swapping .app bundle"
                );
                // Remove old bundle and move new one into place.
                if bundle_path.exists() {
                    std::fs::remove_dir_all(bundle_path)
                        .map_err(|e| format!("failed to remove old bundle: {e}"))?;
                }
                std::process::Command::new("mv")
                    .arg(&source_bundle)
                    .arg(bundle_path)
                    .status()
                    .map_err(|e| format!("failed to mv bundle: {e}"))?;
                return Ok(());
            }
        }

        // Standalone binary fallback: rename the new binary over the current exe.
        let dest = current_exe.clone();
        std::fs::rename(&source_path, &dest)
            .map_err(|e| format!("failed to rename binary: {e}"))?;

        Ok(())
    })();

    match swap_result {
        Ok(()) => {
            tracing::info!(
                target: "gpui_starter::updater",
                version = %version,
                "pending swap applied successfully"
            );
            let _ = std::fs::remove_file(&marker_path);
            super::set_status(UpdateStatus::Idle, cx);
        }
        Err(err) => {
            tracing::error!(
                target: "gpui_starter::updater",
                error = %err,
                "pending swap failed"
            );
            // Leave the marker so the user/admin can investigate, but set error status.
            super::set_status(UpdateStatus::Error(format!("swap failed: {err}")), cx);
        }
    }
}
