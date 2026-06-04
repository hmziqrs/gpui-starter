use tempfile::tempdir;

use super::{SqliteStorage, StorageBackend, init_db};

#[test]
fn initializes_schema_and_migration_table() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("app.db");
    let version = init_db(&db_path).expect("init db");
    assert_eq!(version, 3);

    let conn = rusqlite::Connection::open(&db_path).expect("open db");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 3",
            [],
            |row| row.get(0),
        )
        .expect("read migrations");
    assert_eq!(count, 1);
}

#[test]
fn backend_health_and_maintenance_work() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("app.db");
    init_db(&db_path).expect("init db");
    let backend = SqliteStorage::new(db_path);
    backend.health_check().expect("health check");
    backend.maintenance().expect("maintenance");
    assert_eq!(backend.schema_version().expect("schema version"), 3);
}

#[test]
fn persist_and_load_crash_report_roundtrip() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("test_crash.db");
    init_db(&db_path).expect("init db");
    let backend = SqliteStorage::new(db_path);

    let report = crate::services::crash_report::CrashReport::new(
        "test panic".to_string(),
        "backtrace here".to_string(),
        true,
        vec!["error1".to_string(), "error2".to_string()],
    );

    backend.persist_crash_report(&report).expect("persist crash report");

    let loaded = backend.load_pending_crash_reports(10).expect("load pending");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, report.id);
    assert_eq!(loaded[0].panic_message, "test panic");
    assert_eq!(loaded[0].backtrace, "backtrace here");
    assert!(loaded[0].render_path);
    assert_eq!(loaded[0].recent_errors, vec!["error1", "error2"]);
}

#[test]
fn mark_crash_report_uploaded() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("test_upload.db");
    init_db(&db_path).expect("init db");
    let backend = SqliteStorage::new(db_path);

    let report = crate::services::crash_report::CrashReport::new(
        "panic".to_string(),
        "bt".to_string(),
        false,
        vec![],
    );

    backend.persist_crash_report(&report).expect("persist");
    let pending = backend.load_pending_crash_reports(10).expect("load");
    assert_eq!(pending.len(), 1);

    backend
        .mark_crash_report_uploaded(&report.id, "2025-01-01T00:00:00Z")
        .expect("mark uploaded");

    let pending_after = backend.load_pending_crash_reports(10).expect("load after");
    assert_eq!(pending_after.len(), 0);
}

#[test]
fn load_pending_crash_reports_respects_limit() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("test_limit.db");
    init_db(&db_path).expect("init db");
    let backend = SqliteStorage::new(db_path);

    for _ in 0..5 {
        let report = crate::services::crash_report::CrashReport::new(
            "panic".to_string(),
            "bt".to_string(),
            false,
            vec![],
        );
        backend.persist_crash_report(&report).expect("persist");
    }

    let loaded = backend.load_pending_crash_reports(3).expect("load");
    assert_eq!(loaded.len(), 3);
}
