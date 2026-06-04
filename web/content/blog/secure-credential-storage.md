---
title: "Storing secrets in desktop apps: the keyring approach"
description: "Why storing API keys in config files is a security hole, and how to use the OS keyring from Rust to keep credentials encrypted at rest."
date: 2026-05-19
tags: [Rust, security, desktop]
draft: false
---

Every desktop app that talks to an API needs credentials. API keys, OAuth tokens, database passwords. The question is where to put them. Most apps get this wrong by stuffing secrets into config files or SQLite tables, which means any process on the machine can read them.

## The anti-pattern: secrets in config files and databases

Storing API tokens in a JSON config file or a SQLite table is the path of least resistance. The database is already there. The config file is already being written. Why add another subsystem?

The reason: any process running as the same user can read those files. A malicious script, a compromised dependency, or another app with overly broad file permissions can slurp up every secret your app has stored. SQLite databases are not encrypted by default. JSON config files are plain text. Backup tools copy them to external drives. Crash reporters attach them to tickets. The attack surface compounds fast.

This is not a theoretical concern. Every time a desktop app ships a config file containing `"api_key": "sk-..."`, it has created a plaintext credential dump on the user's disk. Rotating the key later does not help if someone already copied the file.

## How the OS credential store works

Every major operating system ships a secure credential store. On macOS it is Keychain Services, on Windows it is Credential Manager, and on Linux it is Secret Service (backed by libsecret, GNOME Keyring, or KDE Wallet).

These stores encrypt credentials at rest using keys derived from the user's login password. The encryption is handled by the OS kernel or a privileged daemon. Your app never sees the encryption keys. It asks the store to save a value, and later asks to retrieve it. The store handles the cryptography and access control.

On macOS, Keychain Services stores items in the user's login keychain by default. Access can be restricted to a specific code-signed application. On Windows, Credential Manager stores items in the user's profile with DPAPI encryption. On Linux, Secret Service provides a D-Bus interface backed by GNOME Keyring or KDE Wallet, encrypting with the user's login credentials through PAM integration.

What this means in practice: credentials are never written to disk as plaintext. They are encrypted, access-controlled, and managed by a system component the user already trusts.

## The keyring crate

The [keyring](https://crates.io/crates/keyring) crate provides a cross-platform Rust interface to these stores. You create an `Entry` with a service name and a username, then call `set_password`, `get_password`, and `delete_credential`. Under the hood, it calls the native OS API.

gpui-starter uses `keyring = "3.6.3"` with the `apple-native` feature enabled, which talks to the Security.framework directly on macOS rather than shelling out to the `security` CLI tool. This is faster and avoids parsing command output.

The API is straightforward:

```rust
use keyring::Entry;

let entry = Entry::new("my-app", "api-token")?;
entry.set_password("sk-secret-value-here")?;

// Later, retrieve it
let password = entry.get_password()?;

// When the user signs out, delete it
entry.delete_credential()?;
```

If the platform has no credential store available, `Entry::new` returns an error. This is the right behavior. If you cannot store a secret securely, you should know immediately rather than silently falling back to a file.

## The secure_storage module in gpui-starter

gpui-starter wraps the keyring crate in a module called `secure_storage`. It exposes four functions: `initialize`, `set_secret`, `get_secret`, and `delete_secret`. Each function integrates with GPUI's global state and the app's capabilities system.

The `set_secret` function:

```rust
pub fn set_secret(service: &str, key: &str, value: &str, cx: &mut App) -> Result<(), String> {
    let entry = keyring::Entry::new(service, key).map_err(|err| {
        tracing::error!(target: "gpui_starter::secure_storage", "entry creation failed: {err}");
        err.to_string()
    })?;
    entry.set_password(value).map_err(|err| {
        tracing::error!(target: "gpui_starter::secure_storage", service, key, "set_password failed: {err}");
        update_last_error(Some(err.to_string()), cx);
        err.to_string()
    })?;
    tracing::info!(target: "gpui_starter::secure_storage", service, key, "secret written");
    update_last_error(None, cx);
    Ok(())
}
```

The function takes a `service` and `key` pair, writes the value to the OS keyring, and updates the app's global error state. If the keyring is unavailable, the error is recorded in the capabilities system so the settings page can display a degraded status.

On startup, `initialize` probes the keyring by creating a test entry:

```rust
pub fn initialize(cx: &mut App) {
    let available = keyring::Entry::new("gpui-starter", "availability-check").is_ok();
    let snapshot = SecureStorageSnapshot {
        available,
        last_error: if available {
            None
        } else {
            Some("keyring entry initialization unavailable".to_string())
        },
    };
    cx.set_global(snapshot.clone());
    crate::capabilities::set("secure_storage", /* ... */, cx);
}
```

The app knows at launch whether secure storage is functional. If the user is on a headless Linux box without a D-Bus session, the app can adapt its UI accordingly instead of failing silently later.

## The split pattern: references in the database, values in the keyring

The pattern gpui-starter uses is straightforward. Store opaque references in your database. Store the actual secret values in the keyring.

Your SQLite `kv_store` table might hold:

```
key: "github-integration-id"
value: "account_42"
```

The keyring holds:

```
service: "gpui-starter"
username: "github-token-account-42"
password: "ghp_xxxxxxxxxxxx"
```

The database row tells you which integrations exist. The keyring holds the credentials needed to use them. If someone copies the database file, they get a list of integration IDs. Not tokens. Not passwords. Not anything they can authenticate with.

This separation also simplifies backup. You can back up the database without worrying about leaking credentials. You can sync the database across machines without syncing secrets. Each machine's keyring holds its own credentials. We covered a similar separation of concerns in the [SQLite persistence tutorial](/blog/sqlite-rust-desktop-tutorial/) when talking about what data belongs where.

## Export and import flows that redact secrets

When users export their app data, whether for backup, migration, or debugging, secrets must not be included. gpui-starter's export logic writes the database state and config to a JSON file, but it never reads from the keyring during export.

The exported file contains references:

```json
{
  "integrations": [
    { "id": "github-integration-id", "label": "Work GitHub", "has_credential": true }
  ]
}
```

The `has_credential` field tells the import side that a credential exists and needs to be provided. During import, the app prompts the user to re-enter API tokens for each integration marked with `has_credential: true`. This is slightly less convenient than a one-click restore, but it means exported files never contain secrets.

A config file in the wrong hands is an inconvenience. A config file full of API tokens is a security incident. The extra step during import is worth the tradeoff.

## When the keyring is unavailable

On headless Linux servers or CI environments, there may be no keyring daemon running. In that case, `keyring::Entry::new` returns an error, and `initialize` marks secure storage as degraded. The app should handle this gracefully: disable features that require credentials, show a clear message in the UI, and never fall back to storing secrets in a file.

Silent fallback to plaintext is the worst possible behavior. It trains users to ignore security warnings and creates a false sense of safety. If the keyring is unavailable, the right answer is to not store the secret.

## Putting it together

The full pattern: initialize the keyring early, check availability, use the service/key abstraction for namespacing, keep references in your database, and never include secrets in export flows. The `keyring` crate handles the platform differences. Your code stays clean and portable.

For a desktop app that users rely on daily, these guarantees matter. Credentials are the most sensitive data your app handles, and they deserve storage that was actually designed for the job.

See the [architecture overview](/docs/architecture/) for how secure storage fits into gpui-starter's broader module system, or the [secure storage docs](/docs/secure-storage/) for integration specifics. The [Rust desktop security guide](/blog/rust-desktop-security-guide/) covers related hardening techniques.
