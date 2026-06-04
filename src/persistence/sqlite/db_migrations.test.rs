use tempfile::tempdir;

use super::*;

#[test]
fn creates_schema_table_from_empty_db() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let conn = Connection::open(&db_path).expect("open db");

    let version = run_migrations(&conn).expect("run migrations");
    assert_eq!(version, 3);

    let recorded: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .expect("read version");
    assert_eq!(recorded, 3);
}

#[test]
fn is_idempotent() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");

    let conn = Connection::open(&db_path).expect("open db");
    let first = run_migrations(&conn).expect("first run");
    let second = run_migrations(&conn).expect("second run");
    assert_eq!(first, second);

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("count rows");
    assert_eq!(count, 3);
}

#[test]
fn kv_store_table_is_usable() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let conn = Connection::open(&db_path).expect("open db");

    run_migrations(&conn).expect("run migrations");

    conn.execute(
        "INSERT INTO kv_store (key, value, updated_at) VALUES ('k', 'v', datetime('now'))",
        [],
    )
    .expect("insert");

    let value: String = conn
        .query_row("SELECT value FROM kv_store WHERE key = 'k'", [], |row| {
            row.get(0)
        })
        .expect("select");
    assert_eq!(value, "v");
}

#[test]
fn error_log_table_is_usable() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let conn = Connection::open(&db_path).expect("open db");

    run_migrations(&conn).expect("run migrations");

    conn.execute(
        "INSERT INTO error_log (id, occurred_at, severity, category, message, actions)
         VALUES ('test-id', '2025-01-01T00:00:00Z', 'Error', 'Network', 'timeout', \"[]\")",
        [],
    )
    .expect("insert");

    let message: String = conn
        .query_row(
            "SELECT message FROM error_log WHERE id = 'test-id'",
            [],
            |row| row.get(0),
        )
        .expect("select");
    assert_eq!(message, "timeout");
}

#[test]
fn crash_reports_table_is_usable() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let conn = Connection::open(&db_path).expect("open db");

    run_migrations(&conn).expect("run migrations");

    conn.execute(
        "INSERT INTO crash_reports (id, panic_message, backtrace, app_version, os, arch, timestamp, render_path, recent_errors)
         VALUES ('crash-1', 'index out of bounds', 'frame1\nframe2', '0.2.0', 'macos', 'aarch64', '2025-06-01T12:00:00Z', 1, '[\"err1\"]')",
        [],
    )
    .expect("insert crash report");

    let msg: String = conn
        .query_row(
            "SELECT panic_message FROM crash_reports WHERE id = 'crash-1'",
            [],
            |row| row.get(0),
        )
        .expect("select");
    assert_eq!(msg, "index out of bounds");

    let render_path: bool = conn
        .query_row(
            "SELECT render_path FROM crash_reports WHERE id = 'crash-1'",
            [],
            |row| row.get(0),
        )
        .expect("select render_path");
    assert!(render_path);

    let uploaded: bool = conn
        .query_row(
            "SELECT uploaded FROM crash_reports WHERE id = 'crash-1'",
            [],
            |row| row.get(0),
        )
        .expect("select uploaded");
    assert!(!uploaded);
}
