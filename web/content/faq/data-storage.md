---
question: "Where does gpui-starter store app data on disk?"
description: "App data lives in SQLite (WAL mode) and a JSON config file, both under platform-standard directories resolved by the directories crate. Secrets go to the OS keyring instead."
category: "Advanced"
order: 16
---

gpui-starter stores structured data in a local SQLite database running in WAL (write-ahead logging) mode. User preferences go to a separate JSON file. Secrets like API keys never touch the database; they live in the OS keyring. All files go under platform-standard directories, created automatically on first launch.

For how state flows through the app, see the [architecture guide](/docs/architecture/). The [SQLite in Rust desktop apps tutorial](/blog/sqlite-rust-desktop-tutorial/) covers WAL mode, migrations, and query patterns.

## Platform data directories

The `AppPaths` struct at `src/platform/filesystem/paths.rs` uses the `directories` crate (`ProjectDirs::from("com", "gpui-starter", "GPUI Starter")`) to resolve OS-specific locations:

| Directory | macOS path | Purpose |
|-----------|-----------|---------|
| `config_dir` | `~/Library/Application Support/GPUI Starter` | `state.json`, user prefs |
| `data_dir` | `~/Library/Application Support/GPUI Starter` | `app.db`, logs |
| `cache_dir` | `~/Library/Caches/GPUI Starter` | Runtime lock files |
| `log_dir` | `{data_dir}/logs` | App log files |
| `runtime_dir` | `{cache_dir}/runtime` | Single-instance socket |

On Linux these follow the XDG specification (`~/.local/share/gpui-starter/GPUI Starter` for data, `~/.config/gpui-starter/GPUI Starter` for config). All five directories are created with `create_dir_all` on startup, so a fresh install works without any setup.

## SQLite database

The database file is `app.db` inside the data directory (`{data_dir}/app.db`). The `SqliteStorage` struct at `src/services/storage.rs` holds a single `rusqlite::Connection` wrapped in `Arc<Mutex<Connection>>`. Every read and write serializes through that mutex, which is safe because GPUI's model code runs on a single thread anyway.

### WAL mode and concurrency

On initialization, the database runs `PRAGMA journal_mode = WAL`. In WAL mode, readers never block writers and writers never block readers. The app logs errors, records crash reports, and reads KV pairs all from the same connection handle, so concurrent access is common. You can read more about why WAL is the right default in the [performance guide](/docs/performance/).

During shutdown, `maintenance()` runs `PRAGMA optimize` to update internal query planner statistics before the connection closes.

### Tables

The database contains four tables, all created in a single batch during `init_db`:

- **`schema_migrations`** - Tracks which migration versions have been applied. Each row has a `version` (integer PK) and `applied_at` timestamp.
- **`kv_store`** - Simple key/value persistence. Columns: `key` (text PK), `value` (text), `updated_at` (text). Used for small ad-hoc data that does not need its own table.
- **`error_log`** - Records from the error surface system. Indexed on `occurred_at DESC` for fast history queries. See the [diagnostics FAQ](/faq/diagnostics/) for how these errors appear in the UI.
- **`crash_reports`** - Panic backtraces and metadata captured by the crash reporter. Indexed on `timestamp DESC` and `uploaded` (boolean) so the auto-updater can find pending reports quickly.

### Migrations

Migrations live in `src/persistence/sqlite/db_migrations.rs`. Each migration is a numbered step (version 1 = `kv_store`, version 2 = `error_log`, version 3 = `crash_reports`) with a `name` and `up_sql` string. The `run_migrations` function reads `COALESCE(MAX(version), 0)` from `schema_migrations` and runs any migration with a higher version number, recording each one as it goes. To add a new table, append a `Migration` entry to the `MIGRATIONS` array with the next version number.

## Config file (state.json)

User preferences (theme, window bounds, locale, sidebar collapsed state) are persisted as `state.json` in the config directory. Writes are atomic: the app writes to a temp file and renames it into place, so a crash mid-write cannot corrupt the file. If `state.json` becomes unreadable, it gets renamed with a `.bad` extension and the app falls back to hardcoded defaults.

This is separate from the database on purpose. The config file is small (a few KB), changes infrequently, and is easy for users to inspect or edit by hand. The database handles high-frequency structured writes that benefit from indexing.

## Where secrets go

API keys, tokens, and passwords are never stored in SQLite or `state.json`. The [secure storage module](/docs/secure-storage/) delegates to the OS keyring (Keychain on macOS, Secret Service on Linux) using the `keyring` crate. The database only holds opaque service/key references. See the [secure storage FAQ](/faq/secure-storage/) for the full details.

## Blob storage and large files

For binary payloads like images or exported data, store the file in the data directory and keep only the file path in SQLite. This keeps the database small and avoids bloating WAL logs with large binary diffs. The `ensure_parent_dir` helper in `src/platform/filesystem/paths.rs` creates any nested directory structure you need before writing the file.

## Backing up user data

To back up everything, copy two things:

1. `{data_dir}/app.db` (and the accompanying `app.db-wal` and `app.db-shm` files if the app was running recently)
2. `{config_dir}/state.json`

That covers all user data and preferences. Secrets are in the OS keyring, which has its own backup mechanism (Time Machine on macOS, for example).

## Related

- [Architecture guide](/docs/architecture/) for how storage fits into the entity and service model
- [Performance guide](/docs/performance/) for why WAL mode and single-connection locking keep the UI thread responsive
- [SQLite in Rust desktop apps](/blog/sqlite-rust-desktop-tutorial/) for rusqlite patterns
- [Secure storage docs](/docs/secure-storage/) for how the keyring integration works
- [Diagnostics FAQ](/faq/diagnostics/) for how error_log data surfaces in the app
