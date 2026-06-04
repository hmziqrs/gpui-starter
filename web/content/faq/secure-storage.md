---
question: "How do I store passwords and API keys securely in a Rust desktop app?"
description: "gpui-starter stores secrets in the OS keyring (Keychain on macOS, Credential Manager on Windows, Secret Service on Linux) using the keyring crate, never in SQLite or config files."
category: "Features"
order: 13
---

gpui-starter never writes secrets to SQLite, JSON config files, or log output. API keys, tokens, passwords, and signing keys all go through the OS credential store. The [`keyring`](https://crates.io/crates/keyring) crate (v3.6.3) handles the platform-specific details.

The full API is documented in the [secure storage guide](/docs/secure-storage/). This FAQ covers the reasoning and the common questions.

## Why not encrypt and store in the database?

You could. It adds complexity for no real security gain. If you encrypt a secret and store the ciphertext in SQLite, the decryption key has to live somewhere accessible to the app. On a user's machine, "somewhere accessible" means findable. An attacker with disk access will locate the key.

The OS keyring avoids this problem. macOS Keychain stores credentials in a dedicated enclave backed by the Secure Enclave Chip on Apple Silicon. Windows Credential Manager uses DPAPI tied to the user's Windows login. These are not unbreakable, but they raise the bar well above "look in the app data folder."

If you are coming from Electron, this is the same approach `keytar` and `safeStorage` use. The Rust implementation is just thinner and has no native module compilation step.

## How it works

The `secure_storage` module wraps the keyring with four operations:

```rust
use crate::secure_storage;

// Write
secure_storage::set_secret("gpui-starter", "api-token", &token, cx)?;

// Read
let value = secure_storage::get_secret("gpui-starter", "api-token", cx)?;
// Returns Ok(Some(value)), Ok(None) if missing, or Err on keyring failure

// Delete
secure_storage::delete_secret("gpui-starter", "api-token", cx)?;

// Check health
let snap = secure_storage::snapshot(cx);
if !snap.available {
    // keyring is unreachable, degrade gracefully
}
```

On startup, `secure_storage::initialize(cx)` probes the keyring with a test entry. If the probe fails, the module marks itself as degraded in the capability registry. The diagnostics view picks this up and shows the user what went wrong. See the [architecture doc](/docs/architecture/) for how the capability registry works.

## What the database actually stores

SQLite stores a reference row, not the secret:

```sql
-- CORRECT: reference only
INSERT INTO secret_refs (id, service, key)
VALUES (?, 'gpui-starter', 'prod-api-token');

-- WRONG: secret value in the database
INSERT INTO environments (name, api_token)
VALUES ('prod', 'sk-abc123');
```

At runtime, you look up the reference row, then call `get_secret` with the stored service and key. This separation also solves the export problem: when you export config or telemetry data, you ship the reference without ever touching the secret value. The [data storage FAQ](/faq/data-storage/) covers the rest of what goes into SQLite.

## Cross-platform backends

| Platform | Backend | Notes |
|----------|---------|-------|
| macOS | Keychain Services | Uses the `apple-native` feature. Backed by Secure Enclave on Apple Silicon. |
| Windows | Credential Manager | DPAPI-encrypted, tied to the user's Windows login. |
| Linux | Secret Service API (D-Bus) | Works with GNOME Keyring and KDE Wallet. |

The crate detects the platform at compile time. No runtime configuration needed. On Linux without a running keyring daemon (headless server, no D-Bus session), the availability check fails and the module enters degraded mode. It does not fall back to writing secrets to disk.

## Logging policy

All `tracing` calls log the `service` and `key` fields but never the secret value:

```rust
tracing::info!(
    target: "gpui_starter::secure_storage",
    service,
    key,
    "secret written"
);
```

If you extend the module, follow the same rule. The [configuration patterns blog post](/blog/rust-desktop-configuration-patterns/) covers a similar principle for config data.

## Testing without the real keyring

Unit tests use `FakeSecureStorage`, an in-memory stand-in with no OS keyring dependency. It works in both `#[test]` and `#[gpui::test]` contexts:

```rust
let mut storage = FakeSecureStorage::default();
storage.set("token");
assert_eq!(storage.get().as_deref(), Some("token"));
storage.delete();
assert_eq!(storage.get(), None);
```

For integration tests, you can inject a degraded `SecureStorageSnapshot` into the GPUI context instead of probing the real keyring. The [testing guide](/docs/testing/) has the full setup.

## When people migrate from Electron

If you are moving from Electron's `safeStorage` API, the pattern maps directly. The main difference is error handling: Rust forces you to handle the keyring-unavailable case explicitly rather than catching a thrown exception. The [Electron to Rust migration post](/blog/migrating-electron-to-rust/) walks through the broader differences including this one.
