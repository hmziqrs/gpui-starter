mod backend;
mod runtime;

#[cfg(test)]
pub(crate) use backend::SqliteStorage;
pub use runtime::*;

use gpui::Global;

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
    fn persist_crash_report(
        &self,
        report: &crate::services::crash_report::CrashReport,
    ) -> rusqlite::Result<()>;
    fn load_pending_crash_reports(
        &self,
        limit: usize,
    ) -> rusqlite::Result<Vec<crate::services::crash_report::CrashReport>>;
    fn mark_crash_report_uploaded(&self, id: &str, uploaded_at: &str) -> rusqlite::Result<()>;
}

#[cfg(test)]
#[path = "../storage.test.rs"]
mod storage_test;
