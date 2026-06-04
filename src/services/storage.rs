use std::{path::PathBuf, sync::Arc};

use gpui::{App, BorrowAppContext as _, Global};
use rusqlite::Connection;
use std::sync::Mutex;

#[derive(Clone, Debug, Default)]
pub struct StorageSnapshot {
    pub available: bool,
    pub db_path: String,
    pub schema_version: i64,
    pub healthy: bool,
    pub last_maintenance_at: Option<String>,
    pub last_migration_result: Option<String>,
    pub last_error: Option<String>,
}

impl Global for StorageSnapshot {}

pub trait StorageBackend: Send + Sync {
    fn schema_version(&self) -> rusqlite::Result<i64>;
    fn health_check(&self) -> rusqlite::Result<()>;
    fn maintenance(&self) -> rusqlite::Result<()>;
    fn persist_error_record(
        &self,
        error: &crate::error_surface::ErrorRecord,
    ) -> rusqlite::Result<()>;
    fn load_error_history(
        &self,
        limit: usize,
    ) -> rusqlite::Result<Vec<crate::error_surface::ErrorRecord>>;
}

/// SQLite-backed storage that holds a single shared connection rather than
/// opening a new one on every operation. The connection is wrapped in
/// `Arc<Mutex<Connection>>` because `rusqlite::Connection` is `Send` but not
/// `Sync`, so every call serialises behind the mutex automatically.
#[derive(Clone, Debug)]
pub(crate) struct SqliteStorage {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStorage {
    /// Open (or create) the database at `path` and return a storage handle
    /// that reuses the same connection for all future operations.
    ///
    /// # Errors
    ///
    /// Returns `rusqlite::Error` if the database file cannot be opened.
    pub(crate) fn new(path: PathBuf) -> Self {
        let conn = Connection::open(&path).expect("failed to open sqlite connection");
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    /// Constructor for unit tests that need a `SqliteStorage` pointing at an
    /// arbitrary path (bypasses the normal app-state path resolution).
    #[cfg(test)]
    pub fn new_for_test(path: PathBuf) -> Self {
        let conn = Connection::open(&path).expect("failed to open sqlite connection for test");
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    /// Acquire the shared connection lock.
    ///
    /// Callers should hold the lock only for the minimum time needed for
    /// their query and drop it immediately afterwards so other tasks are
    /// not blocked.
    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("storage connection mutex poisoned")
    }
}

impl StorageBackend for SqliteStorage {
    fn schema_version(&self) -> rusqlite::Result<i64> {
        let conn = self.conn();
        conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
    }

    fn health_check(&self) -> rusqlite::Result<()> {
        let conn = self.conn();
        let _: i64 = conn.query_row("SELECT 1", [], |row| row.get(0))?;
        Ok(())
    }

    fn maintenance(&self) -> rusqlite::Result<()> {
        let conn = self.conn();
        conn.execute_batch("PRAGMA optimize;")
    }

    fn persist_error_record(
        &self,
        error: &crate::error_surface::ErrorRecord,
    ) -> rusqlite::Result<()> {
        let conn = self.conn();
        let actions_json = serde_json::to_string(&error.actions)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        conn.execute(
            "INSERT OR IGNORE INTO error_log (id, occurred_at, severity, category, message, actions)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                error.id.to_string(),
                error.occurred_at.to_rfc3339(),
                serde_json::to_string(&error.severity)
                    .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
                    .trim_matches('"'),
                error.category.label(),
                error.message,
                actions_json,
            ],
        )?;
        Ok(())
    }

    fn load_error_history(
        &self,
        limit: usize,
    ) -> rusqlite::Result<Vec<crate::error_surface::ErrorRecord>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, occurred_at, severity, category, message, actions
             FROM error_log
             ORDER BY occurred_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], |row| {
                let id_str: String = row.get(0)?;
                let occurred_at_str: String = row.get(1)?;
                let severity_str: String = row.get(2)?;
                let category_str: String = row.get(3)?;
                let message: String = row.get(4)?;
                let actions_json: String = row.get(5)?;
                Ok((
                    id_str,
                    occurred_at_str,
                    severity_str,
                    category_str,
                    message,
                    actions_json,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut records = Vec::with_capacity(rows.len());
        for (id_str, occurred_at_str, severity_str, category_str, message, actions_json) in rows {
            let id = uuid::Uuid::parse_str(&id_str)
                .map(crate::ids::EventId)
                .map_err(|err| rusqlite::Error::InvalidParameterName(err.to_string()))?;
            let occurred_at = chrono::DateTime::parse_from_rfc3339(&occurred_at_str)
                .map(|dt| crate::time::AppTimestamp(dt.to_utc()))
                .map_err(|err| rusqlite::Error::InvalidParameterName(err.to_string()))?;
            let severity: crate::errors::AppErrorSeverity =
                serde_json::from_str(&format!("\"{severity_str}\""))
                    .map_err(|err| rusqlite::Error::InvalidParameterName(err.to_string()))?;
            let category = match category_str.as_str() {
                "network" | "Network" => crate::error_surface::ErrorCategory::Network,
                "storage" | "Storage" => crate::error_surface::ErrorCategory::Storage,
                "rendering" | "Rendering" => crate::error_surface::ErrorCategory::Rendering,
                "config" | "Config" => crate::error_surface::ErrorCategory::Config,
                "system" | "System" => crate::error_surface::ErrorCategory::System,
                _ => {
                    return Err(rusqlite::Error::InvalidParameterName(format!(
                        "unknown error category `{category_str}`"
                    )));
                }
            };
            let actions: Vec<crate::error_surface::ErrorAction> =
                serde_json::from_str(&actions_json)
                    .map_err(|err| rusqlite::Error::InvalidParameterName(err.to_string()))?;
            records.push(crate::error_surface::ErrorRecord {
                id,
                occurred_at,
                severity,
                category,
                message,
                actions,
            });
        }
        Ok(records)
    }
}

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
        let result = bg
            .spawn(async move { backend.health_check() })
            .await;
        let version_result = bg
            .spawn(async move { backend_clone.schema_version() })
            .await;

        let _ = cx.update(|cx| {
            cx.update_global::<StorageSnapshot, _>(|snap, _cx| {
                match result {
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
        let result = bg
            .spawn(async move { backend.maintenance() })
            .await;

        let _ = cx.update(|cx| {
            cx.update_global::<StorageSnapshot, _>(|snap, _cx| {
                match result {
                    Ok(()) => {
                        snap.last_maintenance_at = Some(chrono::Utc::now().to_rfc3339());
                        snap.last_error = None;
                    }
                    Err(err) => {
                        snap.last_error = Some(err.to_string());
                    }
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

fn init_db(path: &PathBuf) -> rusqlite::Result<i64> {
    let conn = Connection::open(path)?;
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
    "#,
    )?;

    let current_version = 2_i64;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, datetime('now'))",
        [current_version],
    )?;
    Ok(current_version)
}

#[cfg(test)]
#[path = "storage.test.rs"]
mod storage_test;
