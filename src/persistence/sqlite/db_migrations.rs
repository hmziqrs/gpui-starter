use anyhow::Result;
use rusqlite::Connection;

struct Migration {
    version: u32,
    name: &'static str,
    up_sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "kv_store",
        up_sql: r#"
            CREATE TABLE IF NOT EXISTS kv_store (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
        "#,
    },
    Migration {
        version: 2,
        name: "error_log",
        up_sql: r#"
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
    },
    Migration {
        version: 3,
        name: "crash_reports",
        up_sql: r#"
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
    },
];

pub fn run_migrations(conn: &Connection) -> Result<u32> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )?;

    let current_version: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    tracing::info!(
        target: "gpui_starter::db_migrations",
        current_version,
        pending = MIGRATIONS.len().saturating_sub(current_version as usize),
        "checking migrations"
    );

    let mut applied = current_version;

    for migration in MIGRATIONS {
        if migration.version <= current_version {
            continue;
        }

        tracing::info!(
            target: "gpui_starter::db_migrations",
            version = migration.version,
            name = migration.name,
            "applying migration"
        );

        let tx = conn.unchecked_transaction()?;

        tx.execute_batch(migration.up_sql)?;

        tx.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, datetime('now'))",
            [migration.version],
        )?;

        tx.commit()?;

        tracing::info!(
            target: "gpui_starter::db_migrations",
            version = migration.version,
            name = migration.name,
            "migration applied"
        );

        applied = migration.version;
    }

    tracing::info!(
        target: "gpui_starter::db_migrations",
        schema_version = applied,
        "migrations complete"
    );

    Ok(applied)
}

#[cfg(test)]
#[path = "db_migrations.test.rs"]
mod db_migrations_test;
