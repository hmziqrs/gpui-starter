---
title: "Using SQLite in a Rust desktop app: a practical guide"
description: "How to integrate SQLite into a Rust desktop app with rusqlite. Schema design, WAL mode, migrations, and query patterns for local-first apps."
date: 2026-06-05
tags: [Rust, SQLite, tutorial]
draft: false
---

SQLite gets dismissed a lot in Rust circles. People reach for Postgres drivers or key-value stores before considering the database that already lives on every user's machine. For a desktop app, SQLite is usually the right call. It handles concurrent reads through WAL mode, stores a single file the user can back up, and adds about 1 MB to your binary when bundled with rusqlite.

This guide covers how we set up SQLite in a GPUI desktop app: connection management, schema migrations, WAL mode tuning, and the query patterns that actually work when your storage layer sits behind `Arc<Mutex<Connection>>`.

If you want the broader picture of how persistence fits into the app architecture, see the [architecture docs](/docs/docs/architecture/).

## Why SQLite for desktop

Desktop apps have different constraints than web servers. You have one user, one machine, and you want reads to be fast because every frame counts. SQLite excels here because there is no network hop. The database file sits on the same disk as the app. Reads complete in microseconds, not milliseconds.

The tradeoff: SQLite is a single-writer database. Only one write transaction can run at a time. For a desktop app with one user, this rarely matters. Your write volume is low (saving preferences, logging errors, caching data). If you need bulk writes, use a transaction and batch them.

## Setting up rusqlite

Add rusqlite to your `Cargo.toml` with the `bundled` feature so you do not need SQLite installed on the build machine:

```toml
[dependencies]
rusqlite = { version = "0.32", features = ["bundled"] }
```

The bundled feature compiles SQLite from C source as part of your crate build. It adds compile time (about 30 seconds on a modern machine) but eliminates the "SQLite not found" problem on user machines.

## Connection management

`rusqlite::Connection` is `Send` but not `Sync`. In a GPUI app where multiple async tasks might want database access, you need a synchronization wrapper. We use `Arc<Mutex<Connection>>`:

```rust
use std::sync::{Arc, Mutex};
use rusqlite::Connection;

pub struct SqliteStorage {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStorage {
    pub fn new(path: PathBuf) -> Self {
        let conn = Connection::open(&path)
            .expect("failed to open sqlite connection");
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock()
            .expect("storage connection mutex poisoned")
    }
}
```

Every storage method acquires the lock, does its work, and returns. The `MutexGuard` drops at the end of the method, releasing the lock. This serializes all database access, which sounds bad but is fine for a single-user desktop app. Your writes are infrequent and your reads are fast.

One important detail: if a thread panics while holding the lock, the mutex becomes "poisoned." The `expect()` call above will panic on the next access. For a desktop app, this is the right behavior because a poisoned mutex means your database is in an unknown state. Crash, restart, recover.

## WAL mode

SQLite's default journal mode is DELETE. On every write, SQLite creates a rollback journal file, writes the original data there, then modifies the database file. On commit, it deletes the journal. This works but prevents concurrent reads during writes.

WAL (Write-Ahead Logging) mode changes the mechanics. Writes go to a separate WAL file. Readers continue reading from the original database file. A checkpoint process merges the WAL back into the main file periodically. This means reads never block on writes, which matters when your UI thread needs to query preferences while a background task logs an error.

Enable it once when you open the database:

```rust
let conn = Connection::open(&path)?;
conn.execute_batch("PRAGMA journal_mode = WAL;")?;
```

SQLite remembers WAL mode across connections. You set it once and it persists in the database file. But I enable it in the initialization code anyway as a defensive measure, in case the user replaces the file.

Two more PRAGMAs worth setting:

```rust
conn.execute_batch("
    PRAGMA journal_mode = WAL;
    PRAGMA busy_timeout = 5000;
    PRAGMA foreign_keys = ON;
")?;
```

`busy_timeout` tells SQLite to retry for up to 5 seconds when the database is locked by another writer, instead of failing immediately. `foreign_keys` enforces referential integrity (off by default for backwards compatibility).

## Schema migrations

Hard-coding `CREATE TABLE IF NOT EXISTS` statements works for a prototype. Once you ship version 2 and need to add a column, you need migrations. We use a versioned migration system:

```rust
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
];
```

The migration runner reads the current version from a `schema_migrations` table, then applies any migration with a version number higher than the current one:

```rust
pub fn run_migrations(conn: &Connection) -> Result<u32> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );"
    )?;

    let current_version: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    for migration in MIGRATIONS {
        if migration.version <= current_version {
            continue;
        }

        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(migration.up_sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (version, applied_at)
             VALUES (?1, datetime('now'))",
            [migration.version],
        )?;
        tx.commit()?;
    }

    Ok(current_version)
}
```

Each migration runs in its own transaction. If migration 3 fails, migration 2 is still committed. The next app launch will retry migration 3. Using `unchecked_transaction()` skips the automatic begin check and is slightly faster, though regular `transaction()` works fine too.

For more on how migrations fit into the app lifecycle, see the [getting started guide](/docs/docs/getting-started/).

## Query patterns

### Inserts with rusqlite::params

Use the `params!` macro for parameterized queries. It handles type conversion and prevents SQL injection:

```rust
fn persist_error_record(&self, error: &ErrorRecord) -> rusqlite::Result<()> {
    let conn = self.conn();
    conn.execute(
        "INSERT OR IGNORE INTO error_log
         (id, occurred_at, severity, category, message, actions)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            error.id.to_string(),
            error.occurred_at.to_rfc3339(),
            error.severity_label(),
            error.category.label(),
            error.message,
            serde_json::to_string(&error.actions)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
        ],
    )?;
    Ok(())
}
```

`INSERT OR IGNORE` skips the row if the primary key already exists. This makes the operation idempotent. You can call it twice with the same error record and nothing breaks.

### Reads with query_map

For multi-row reads, `query_map` gives you an iterator instead of loading everything into memory:

```rust
fn load_error_history(&self, limit: usize) -> rusqlite::Result<Vec<ErrorRecord>> {
    let conn = self.conn();
    let mut stmt = conn.prepare(
        "SELECT id, occurred_at, severity, category, message, actions
         FROM error_log
         ORDER BY occurred_at DESC
         LIMIT ?1"
    )?;

    let rows = stmt
        .query_map([limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Parse raw strings into domain types...
    Ok(records)
}
```

I read everything as `String` first, then parse into domain types in a second pass. This is deliberate. Mixing `rusqlite::Result` with your domain parsing in a single closure makes error handling messy. Two clean passes is easier to debug when something goes wrong.

### Scalar queries

For single values, `query_row` is the tool:

```rust
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
```

The health check runs `SELECT 1`. If the connection is broken or the file is corrupted, this query fails and the caller knows storage is unavailable. The `COALESCE` in the schema version query handles the case where the migrations table is empty (fresh database).

## JSON in SQLite columns

SQLite has no native array or map type. When you need to store structured data (error action lists, crash report metadata), serialize to JSON and store as TEXT:

```rust
let actions_json = serde_json::to_string(&error.actions)
    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
conn.execute(
    "INSERT INTO error_log (..., actions) VALUES (..., ?1)",
    rusqlite::params![actions_json],
)?;
```

On read, parse it back:

```rust
let actions: Vec<ErrorAction> = serde_json::from_str(&actions_json)
    .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;
```

This works well for data you read and write as a whole unit. If you need to query inside the JSON (find all errors with a specific action type), consider extracting that field into its own column with an index instead.

## Background operations

SQLite operations are synchronous. A `query_map` call blocks the thread it runs on. In a GUI app, that thread should not be the render thread. GPUI provides a background executor for this:

```rust
pub fn run_health_check(cx: &mut App) {
    let runtime = cx.try_global::<StorageRuntime>().cloned();
    let Some(runtime) = runtime else { return };
    let bg = cx.background_executor().clone();

    cx.spawn(async move |cx| {
        let backend = runtime.backend.clone();
        let result = bg.spawn(async move {
            backend.health_check()
        }).await;

        let _ = cx.update(|cx| {
            cx.update_global::<StorageSnapshot, _>(|snap, _cx| {
                match result {
                    Ok(()) => {
                        snap.healthy = true;
                        snap.last_error = None;
                    }
                    Err(err) => {
                        snap.healthy = false;
                        snap.last_error = Some(err.to_string());
                    }
                }
            });
        });
    }).detach();
}
```

The pattern: clone what you need, spawn on the background executor, then use `cx.update` to write results back to the global state on the main thread. GPUI's `spawn` handles the thread hopping for you.

## Maintenance

SQLite needs occasional maintenance. The `PRAGMA optimize` command tells the query planner to analyze your indexes and update statistics. Run it when the app shuts down or during idle time:

```rust
fn maintenance(&self) -> rusqlite::Result<()> {
    let conn = self.conn();
    conn.execute_batch("PRAGMA optimize;")
}
```

For a desktop app with modest write volume, running this once per session is sufficient.

## What we skipped

This guide does not cover SQLite's full-text search extension (compile with `features = ["bundled-sqlcipher"]` or `features = ["bundled", "fts5"]`), the `r2d2` connection pool (unnecessary for single-connection desktop apps), or ORM-style wrappers like `diesel` and `sqlx` (valid choices, but rusqlite is lower-level and matches the explicit style of GPUI apps).

For a working implementation of everything covered here, check out the [gpui-starter repository](https://github.com/freeoxide/gpui-starter). The storage module uses the exact patterns described in this post: `Arc<Mutex<Connection>>`, versioned migrations, WAL mode, and background health checks. The [getting started docs](/docs/docs/getting-started/) walk through cloning and running the project locally.
