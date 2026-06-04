---
title: "Secure storage"
description: "Store credentials in the OS keyring from a GPUI app using the keyring crate with opaque database references"
---

## How secure storage works

gpui-starter stores secrets (API tokens, passwords, signing keys) in the operating system credential store, not in SQLite or log files. The `secure_storage` module at `src/services/secure_storage.rs` wraps the [keyring](https://crates.io/crates/keyring) crate (v3.6.3) and exposes four functions: `initialize`, `snapshot`, `set_secret`, `get_secret`, and `delete_secret`.

The database only holds opaque references (a service name and key pair). The actual secret values live in the platform keyring. If you are new to the project, start with the [getting started](/docs/getting-started/) guide to get the app running, then come back here.

## Platform credential stores

The `keyring` crate delegates to the native credential manager on each platform. No extra configuration is needed; the correct backend is selected at compile time.

| Platform | Backend |
|----------|---------|
| macOS | Keychain Services (via `apple-native` feature) |
| Linux | Secret Service API (GNOME Keyring, KDE Wallet) |
| Windows | Credential Manager |

On Linux, the user must have a keyring daemon running (GNOME Keyring or KDE Wallet). If no daemon is available, the availability check will fail and the module degrades gracefully.

## Availability check and degraded mode

On startup, `secure_storage::initialize(cx)` probes the keyring by creating a test entry:

```rust
let available = keyring::Entry::new("gpui-starter", "availability-check").is_ok();
```

If the probe fails, the module registers itself as degraded in the capability registry. The global `SecureStorageSnapshot` tracks whether the keyring is reachable and records the last error. You can check it at any point:

```rust
let snap = secure_storage::snapshot(cx);
if !snap.available {
    // keyring is unreachable, show degraded UI
}
```

The capability registry entry (`"secure_storage"`) exposes `supported`, `enabled`, `degraded`, and `last_error` fields. The diagnostics view reads these fields to surface keyring problems to the user. See the [architecture](/docs/architecture/) doc for how the capability registry fits into the broader state model.

## API reference

All functions that mutate state take `&mut App` so they can update the global error state.

### `initialize`

```rust
pub fn initialize(cx: &mut App)
```

Called once during app startup. Probes the keyring, sets the global `SecureStorageSnapshot`, and registers the capability. Does not return a value. If the keyring probe fails, the snapshot records the error and the capability is marked degraded.

### `snapshot`

```rust
pub fn snapshot(cx: &App) -> SecureStorageSnapshot
```

Returns a clone of the current keyring state. Available at any time after initialization. Returns a default (unavailable) snapshot if `initialize` has not been called.

### `set_secret`

```rust
pub fn set_secret(
    service: &str,
    key: &str,
    value: &str,
    cx: &mut App,
) -> Result<(), SecureStorageError>
```

Writes a secret under the given service and key. On success, clears `last_error` and returns `Ok(())`. On failure, logs through `tracing` and returns a `SecureStorageError` variant with the service, key, and original `keyring::Error`.

### `get_secret`

```rust
pub fn get_secret(
    service: &str,
    key: &str,
    cx: &mut App,
) -> Result<Option<SharedString>, SecureStorageError>
```

Returns `Some(value)` if the entry exists. Returns `None` if the entry does not exist (`keyring::Error::NoEntry`). Returns an error on read failure (keyring unreachable, permission denied, etc.).

### `delete_secret`

```rust
pub fn delete_secret(
    service: &str,
    key: &str,
    cx: &mut App,
) -> Result<(), SecureStorageError>
```

Deletes the credential from the OS keyring. Fails if the entry does not exist or the keyring is unreachable. Clears `last_error` on success.

## Error types

The module uses a typed error enum rather than string errors. Each variant carries the `service` and `key` that caused the failure, plus the original `keyring::Error` as the source:

```rust
pub enum SecureStorageError {
    EntryCreation { service: String, key: String, source: keyring::Error },
    SetFailed { service: String, key: String, source: keyring::Error },
    GetFailed { service: String, key: String, source: keyring::Error },
    DeleteFailed { service: String, key: String, source: keyring::Error },
}
```

This lets callers match on specific failure modes and display targeted UI messages. The error types also integrate with the notification system described in the [notifications](/docs/notifications/) doc.

## The reference pattern: opaque IDs in SQLite

The architecture separates secret values from structured data. The database stores references; the keyring holds the values.

```sql
-- WRONG: secret value in the database
INSERT INTO environments (name, api_token) VALUES ('prod', 'sk-abc123');

-- CORRECT: opaque reference in the database
INSERT INTO secret_refs (id, service, key) VALUES (?, 'gpui-starter', 'prod-api-token');
-- actual value lives in the OS keyring under service='gpui-starter', key='prod-api-token'
```

To read a secret at runtime, look up the reference row from SQLite, then call `get_secret` with the stored service and key. This pattern is also documented in the [architecture](/docs/architecture/) guide under the "Secrets" section.

## Using secrets in a view

A complete example of writing and reading a secret from a settings page, with error handling wired to the notification system:

```rust
use crate::secure_storage;
use crate::notifications;

fn save_api_token(token: &str, window: &mut Window, cx: &mut Context<SettingsView>) {
    let message = match secure_storage::set_secret(
        "gpui-starter",
        "api-token",
        token,
        cx,
    ) {
        Ok(()) => "API token saved to keyring".to_string(),
        Err(err) => format!("Failed to save token: {err}"),
    };
    notifications::push(message, window, cx);
}

fn load_api_token(cx: &mut Context<SettingsView>) -> Option<String> {
    secure_storage::get_secret("gpui-starter", "api-token", cx)
        .ok()
        .flatten()
        .map(|s| s.to_string())
}
```

The `set_secret` call updates the global `last_error` field automatically, so the diagnostics page reflects keyring health without any extra wiring from the caller.

## Logging policy

Every function logs through `tracing` with the target `gpui_starter::secure_storage`. The log messages include the `service` and `key` fields but never the secret value itself:

```rust
tracing::info!(
    target: "gpui_starter::secure_storage",
    service,
    key,
    "secret written"
);
```

If you add new code that touches secrets, follow the same rule: log the identifier, not the payload.

## Testing without the OS keyring

The `testing` module provides `FakeSecureStorage` for unit tests that should not touch the OS keyring. It is a plain struct with no GPUI dependency, so it works in both `#[test]` and `#[gpui::test]` contexts. See the [testing](/docs/testing/) guide for the full test setup reference.

```rust
#[derive(Default)]
pub struct FakeSecureStorage {
    value: Option<String>,
}

impl FakeSecureStorage {
    pub fn set(&mut self, value: &str) { self.value = Some(value.to_string()); }
    pub fn get(&self) -> Option<String> { self.value.clone() }
    pub fn delete(&mut self) { self.value = None; }
}
```

Usage in a test:

```rust
#[test]
fn fake_storage_roundtrip() {
    let mut storage = FakeSecureStorage::default();
    storage.set("token");
    assert_eq!(storage.get().as_deref(), Some("token"));
    storage.delete();
    assert_eq!(storage.get(), None);
}
```

For tests that need the full GPUI context, you can set a fake global snapshot instead of probing the real keyring:

```rust
#[gpui::test]
fn test_with_degraded_keyring(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(SecureStorageSnapshot {
            available: false,
            last_error: Some("keyring entry initialization unavailable".into()),
        });
    });
    // code under test sees degraded state
}
```

## Export and import: redacting secrets

When exporting application data (telemetry config, environment snapshots), secrets must never appear in the output. The telemetry module demonstrates the redaction pattern:

```rust
fn redact_endpoint(endpoint: &str) -> Option<String> {
    let host = endpoint
        .split("://")
        .nth(1)
        .unwrap_or(endpoint)
        .split('/')
        .next()
        .unwrap_or(endpoint);
    Some(format!("{host}/..."))
}
```

Three rules for export/import flows:

1. Replace secret values with placeholders or omit them entirely.
2. Store only the service/key reference pair so the imported config can rebind secrets on the target machine.
3. Never log raw secret values. The `tracing` calls in `secure_storage.rs` log the service and key, never the value.

## Function reference

| Function | Returns | Failure mode |
|----------|---------|--------------|
| `initialize(cx)` | Sets global `SecureStorageSnapshot` and capability | Marks degraded if keyring probe fails |
| `snapshot(cx)` | `SecureStorageSnapshot` | Returns default if not initialized |
| `set_secret(service, key, value, cx)` | `Result<(), SecureStorageError>` | Error on entry creation or write failure |
| `get_secret(service, key, cx)` | `Result<Option<SharedString>, SecureStorageError>` | `None` if no entry, error on read failure |
| `delete_secret(service, key, cx)` | `Result<(), SecureStorageError>` | Error if entry missing or keyring unreachable |
