use std::{path::PathBuf, sync::Arc};

use rusqlite::Connection;
use std::sync::Mutex;

use super::StorageBackend;

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

    fn persist_crash_report(
        &self,
        report: &crate::services::crash_report::CrashReport,
    ) -> rusqlite::Result<()> {
        let conn = self.conn();
        let recent_errors_json = serde_json::to_string(&report.recent_errors)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        conn.execute(
            "INSERT OR IGNORE INTO crash_reports (id, panic_message, backtrace, app_version, os, arch, timestamp, render_path, recent_errors)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                report.id,
                report.panic_message,
                report.backtrace,
                report.app_version,
                report.os,
                report.arch,
                report.timestamp,
                report.render_path,
                recent_errors_json,
            ],
        )?;
        Ok(())
    }

    fn load_pending_crash_reports(
        &self,
        limit: usize,
    ) -> rusqlite::Result<Vec<crate::services::crash_report::CrashReport>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, panic_message, backtrace, app_version, os, arch, timestamp, render_path, recent_errors
             FROM crash_reports
             WHERE uploaded = 0
             ORDER BY timestamp DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], |row| {
                let id: String = row.get(0)?;
                let panic_message: String = row.get(1)?;
                let backtrace: String = row.get(2)?;
                let app_version: String = row.get(3)?;
                let os: String = row.get(4)?;
                let arch: String = row.get(5)?;
                let timestamp: String = row.get(6)?;
                let render_path: bool = row.get(7)?;
                let recent_errors_json: String = row.get(8)?;
                Ok((
                    id,
                    panic_message,
                    backtrace,
                    app_version,
                    os,
                    arch,
                    timestamp,
                    render_path,
                    recent_errors_json,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut reports = Vec::with_capacity(rows.len());
        for (id, panic_message, backtrace, app_version, os, arch, timestamp, render_path, recent_errors_json) in rows {
            let recent_errors: Vec<String> = serde_json::from_str(&recent_errors_json)
                .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
            reports.push(crate::services::crash_report::CrashReport {
                id,
                panic_message,
                backtrace,
                app_version,
                os,
                arch,
                timestamp,
                render_path,
                recent_errors,
            });
        }
        Ok(reports)
    }

    fn mark_crash_report_uploaded(&self, id: &str, uploaded_at: &str) -> rusqlite::Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE crash_reports SET uploaded = 1, uploaded_at = ?1 WHERE id = ?2",
            rusqlite::params![uploaded_at, id],
        )?;
        Ok(())
    }
}
