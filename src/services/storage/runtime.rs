use std::{path::PathBuf, sync::Arc};

use gpui::{App, BorrowAppContext as _, Global};

use super::backend::SqliteStorage;
use super::{StorageBackend, StorageSnapshot};

#[derive(Clone)]
pub struct StorageRuntime {
    pub(crate) backend: Arc<dyn StorageBackend>,
}

impl Global for StorageRuntime {}

pub fn initialize(cx: &mut App) {
    let path = db_path(cx);
    let backend = Arc::new(SqliteStorage::new(path.clone()));
    let mut snapshot = StorageSnapshot {
        db_path: path.display().to_string(),
        ..StorageSnapshot::default()
    };

    match init_db(&path) {
        Ok(schema_version) => {
            snapshot.available = true;
            snapshot.schema_version = schema_version;
            snapshot.last_migration_result =
                Some(format!("schema version {} ready", schema_version));
            tracing::info!(
                target: "gpui_starter::storage",
                db_path = %snapshot.db_path,
                schema_version,
                "storage initialized"
            );
        }
        Err(err) => {
            let error = err.to_string();
            snapshot.last_error = Some(error.clone());
            snapshot.last_migration_result = Some("migration failed".to_string());
            tracing::error!(
                target: "gpui_starter::storage",
                db_path = %snapshot.db_path,
                error = %error,
                "storage initialization failed"
            );
        }
    }

    if snapshot.available {
        match backend.health_check() {
            Ok(()) => snapshot.healthy = true,
            Err(err) => {
                snapshot.healthy = false;
                snapshot.last_error = Some(err.to_string());
            }
        }
    }

    crate::capabilities::set(
        "storage",
        crate::capabilities::CapabilityStatus {
            supported: true,
            enabled: snapshot.available,
            degraded: snapshot.last_error.is_some() || !snapshot.healthy,
            reason: snapshot
                .last_error
                .as_ref()
                .map(|err| format!("storage issue: {err}").into()),
            last_error: snapshot.last_error.clone().map(Into::into),
        },
        cx,
    );

    cx.set_global(snapshot);
    cx.set_global(StorageRuntime { backend });
}

pub fn snapshot(cx: &App) -> StorageSnapshot {
    cx.try_global::<StorageSnapshot>()
        .cloned()
        .unwrap_or_default()
}

/// Run a health check against the storage backend on a background thread so
/// the main GPUI render loop is not blocked by synchronous SQLite I/O.
///
/// The result is written back to the global [`StorageSnapshot`] via an
/// `cx.update` callback once the check completes.
pub fn run_health_check(cx: &mut App) {
    let Some(runtime) = cx.try_global::<StorageRuntime>().cloned() else {
        return;
    };
    let bg = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        let backend = runtime.backend.clone();
        let backend_clone = Arc::clone(&backend);
        let result = bg.spawn(async move { backend.health_check() }).await;
        let version_result = bg
            .spawn(async move { backend_clone.schema_version() })
            .await;

        let _ = cx.update(|cx| {
            cx.update_global::<StorageSnapshot, _>(|snap, _cx| match result {
                Ok(()) => {
                    snap.healthy = true;
                    snap.last_error = None;
                    if let Ok(version) = version_result {
                        snap.schema_version = version;
                    }
                }
                Err(err) => {
                    snap.healthy = false;
                    snap.last_error = Some(err.to_string());
                }
            });
        });
    })
    .detach();
}

/// Run storage maintenance (e.g. `PRAGMA optimize`) on a background thread so
/// the main GPUI render loop is not blocked by synchronous SQLite I/O.
///
/// The result is written back to the global [`StorageSnapshot`] via an
/// `cx.update` callback once maintenance completes.
pub fn run_maintenance(cx: &mut App) {
    let Some(runtime) = cx.try_global::<StorageRuntime>().cloned() else {
        return;
    };
    let bg = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        let backend = runtime.backend.clone();
        let result = bg.spawn(async move { backend.maintenance() }).await;

        let _ = cx.update(|cx| {
            cx.update_global::<StorageSnapshot, _>(|snap, _cx| match result {
                Ok(()) => {
                    snap.last_maintenance_at = Some(chrono::Utc::now().to_rfc3339());
                    snap.last_error = None;
                }
                Err(err) => {
                    snap.last_error = Some(err.to_string());
                }
            });
        });
    })
    .detach();
}

pub fn shutdown(cx: &mut App) {
    let snapshot = snapshot(cx);
    tracing::debug!(
        target: "gpui_starter::storage",
        available = snapshot.available,
        healthy = snapshot.healthy,
        db_path = %snapshot.db_path,
        "storage shutdown requested"
    );
}

fn db_path(cx: &App) -> PathBuf {
    crate::app_state::paths(cx).data_dir.join("app.db")
}

pub(crate) fn init_db(path: &PathBuf) -> rusqlite::Result<i64> {
    let conn = rusqlite::Connection::open(path)?;
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS kv_store (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS error_log (
            id TEXT PRIMARY KEY,
            occurred_at TEXT NOT NULL,
            severity TEXT NOT NULL,
            category TEXT NOT NULL,
            message TEXT NOT NULL,
            actions TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_error_log_occurred_at
            ON error_log (occurred_at DESC);
        CREATE TABLE IF NOT EXISTS crash_reports (
            id TEXT PRIMARY KEY,
            panic_message TEXT NOT NULL,
            backtrace TEXT NOT NULL,
            app_version TEXT NOT NULL,
            os TEXT NOT NULL,
            arch TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            render_path BOOLEAN NOT NULL DEFAULT 0,
            recent_errors TEXT NOT NULL DEFAULT "[]",
            uploaded BOOLEAN NOT NULL DEFAULT 0,
            uploaded_at TEXT,
            upload_error TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_crash_reports_timestamp
            ON crash_reports (timestamp DESC);
        CREATE INDEX IF NOT EXISTS idx_crash_reports_uploaded
            ON crash_reports (uploaded);
    "#,
    )?;

    let current_version = 3_i64;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, datetime('now'))",
        [current_version],
    )?;
    Ok(current_version)
}
