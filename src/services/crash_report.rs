#![allow(dead_code)]

use std::path::Path;

use gpui::{App, BorrowAppContext as _, Global};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// CrashReport data model
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrashReport {
    pub id: String,
    pub panic_message: String,
    pub backtrace: String,
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub timestamp: String,
    pub render_path: bool,
    pub recent_errors: Vec<String>,
}

impl CrashReport {
    pub fn new(
        panic_message: String,
        backtrace: String,
        render_path: bool,
        recent_errors: Vec<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            panic_message,
            backtrace,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            render_path,
            recent_errors,
        }
    }
}

// ---------------------------------------------------------------------------
// GPUI Global snapshot
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct CrashReportSnapshot {
    pub pending_count: usize,
    pub last_crash_timestamp: Option<String>,
    pub upload_endpoint: String,
    pub last_upload_error: Option<String>,
}

impl Global for CrashReportSnapshot {}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

pub fn initialize(cx: &mut App) {
    let endpoint = option_env!("GPUI_CRASH_REPORT_URL")
        .unwrap_or("")
        .to_string();

    let snapshot = CrashReportSnapshot {
        upload_endpoint: endpoint,
        ..CrashReportSnapshot::default()
    };

    cx.set_global(snapshot.clone());

    tracing::info!(
        target: "gpui_starter::crash_report",
        endpoint = %snapshot.upload_endpoint,
        "crash report service initialized"
    );
}

pub fn snapshot(cx: &App) -> CrashReportSnapshot {
    cx.try_global::<CrashReportSnapshot>()
        .cloned()
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// File-based crash report I/O (synchronous, for panic handler)
// ---------------------------------------------------------------------------

/// Write a crash report as JSON to `{data_dir}/crash_reports/{id}.json`.
///
/// This is intentionally synchronous and uses `std::fs` because it is called
/// from the panic hook where async I/O is not available.
pub fn write_crash_report(report: &CrashReport, data_dir: &Path) -> std::io::Result<()> {
    let reports_dir = data_dir.join("crash_reports");
    std::fs::create_dir_all(&reports_dir)?;

    let file_path = reports_dir.join(format!("{}.json", report.id));
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(&file_path, json)?;

    tracing::info!(
        target: "gpui_starter::crash_report",
        path = %file_path.display(),
        "crash report written to disk"
    );

    Ok(())
}

/// Scan a directory for `.json` crash report files and parse them.
///
/// Non-JSON files and malformed entries are silently skipped.
pub fn detect_pending_reports(data_dir: &Path) -> Vec<CrashReport> {
    let reports_dir = data_dir.join("crash_reports");
    if !reports_dir.exists() {
        return Vec::new();
    }

    let Ok(entries) = std::fs::read_dir(&reports_dir) else {
        return Vec::new();
    };

    let mut reports = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        match serde_json::from_str::<CrashReport>(&contents) {
            Ok(report) => reports.push(report),
            Err(err) => {
                tracing::warn!(
                    target: "gpui_starter::crash_report",
                    path = %path.display(),
                    error = %err,
                    "skipping malformed crash report file"
                );
            }
        }
    }

    // Sort by timestamp descending (newest first).
    reports.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    reports
}

// ---------------------------------------------------------------------------
// Upload pending reports (async)
// ---------------------------------------------------------------------------

/// Read pending reports from SQLite, POST each as JSON to the configured
/// endpoint, and mark them as uploaded on success.
///
/// Silently returns when the upload endpoint is empty or the storage backend
/// is not available.
pub fn upload_pending_reports(cx: &mut App) {
    let snap = snapshot(cx);
    if snap.upload_endpoint.is_empty() {
        tracing::debug!(
            target: "gpui_starter::crash_report",
            "no upload endpoint configured, skipping pending report upload"
        );
        return;
    }

    let Some(runtime) = cx.try_global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
    else {
        tracing::warn!(
            target: "gpui_starter::crash_report",
            "tokio runtime not available for crash report upload"
        );
        return;
    };

    let Some(storage_runtime) = cx.try_global::<crate::storage::StorageRuntime>() else {
        tracing::warn!(
            target: "gpui_starter::crash_report",
            "storage runtime not available for crash report upload"
        );
        return;
    };

    let endpoint = snap.upload_endpoint.clone();
    let http_client = runtime.0.http_client.clone();
    let backend = storage_runtime.backend.clone();

    cx.spawn(async move |cx| {
        let reports = {
            let backend = backend.clone();
            cx.background_executor()
                .spawn(async move { backend.load_pending_crash_reports(50) })
                .await
        };

        let reports = match reports {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(
                    target: "gpui_starter::crash_report",
                    error = %err,
                    "failed to load pending crash reports"
                );
                return;
            }
        };

        if reports.is_empty() {
            return;
        }

        tracing::info!(
            target: "gpui_starter::crash_report",
            count = reports.len(),
            "uploading pending crash reports"
        );

        for report in &reports {
            let body = match serde_json::to_string(report) {
                Ok(b) => b,
                Err(err) => {
                    tracing::warn!(
                        target: "gpui_starter::crash_report",
                        id = %report.id,
                        error = %err,
                        "failed to serialize crash report"
                    );
                    continue;
                }
            };

            let result = http_client
                .post(&endpoint)
                .header("Content-Type", "application/json")
                .body(body)
                .send()
                .await;

            match result {
                Ok(resp) if resp.status().is_success() => {
                    let uploaded_at = chrono::Utc::now().to_rfc3339();
                    let backend = backend.clone();
                    let id = report.id.clone();
                    let mark_result = cx
                        .background_executor()
                        .spawn(async move { backend.mark_crash_report_uploaded(&id, &uploaded_at) })
                        .await;
                    if let Err(err) = mark_result {
                        tracing::warn!(
                            target: "gpui_starter::crash_report",
                            id = %report.id,
                            error = %err,
                            "failed to mark crash report as uploaded"
                        );
                    }
                }
                Ok(resp) => {
                    tracing::warn!(
                        target: "gpui_starter::crash_report",
                        id = %report.id,
                        status = %resp.status(),
                        "crash report upload returned non-success status"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        target: "gpui_starter::crash_report",
                        id = %report.id,
                        error = %err,
                        "crash report upload failed"
                    );
                }
            }
        }

        // Update snapshot with latest pending count.
        let backend = backend.clone();
        let backend_for_spawn = backend.clone();
        let _ = cx.update(|cx| {
            let pending = cx
                .background_executor()
                .spawn(async move { backend.load_pending_crash_reports(1) });
            // We can't block here; schedule a background update.
            let backend2 = backend_for_spawn.clone();
            cx.spawn(async move |cx| {
                let count = match pending.await {
                    Ok(r) => r.len(),
                    Err(_) => 0,
                };
                let _ = cx.update(|cx| {
                    cx.update_global::<CrashReportSnapshot, _>(|snap, _cx| {
                        snap.pending_count = count;
                    });
                });
                // Also try to get the latest timestamp
                let backend3 = backend2.clone();
                let latest = cx
                    .background_executor()
                    .spawn(async move { backend3.load_pending_crash_reports(1) })
                    .await;
                if let Ok(r) = latest {
                    let _ = cx.update(|cx| {
                        cx.update_global::<CrashReportSnapshot, _>(|snap, _cx| {
                            snap.last_crash_timestamp = r.first().map(|r| r.timestamp.clone());
                        });
                    });
                }
            })
            .detach();
        });
    })
    .detach();
}

// ---------------------------------------------------------------------------
// Shutdown
// ---------------------------------------------------------------------------

pub fn shutdown(cx: &mut App) {
    tracing::debug!(
        target: "gpui_starter::crash_report",
        "crash report service shutdown requested"
    );
    // Attempt a final flush of pending uploads.
    upload_pending_reports(cx);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "crash_report.test.rs"]
mod crash_report_test;
