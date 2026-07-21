---
title: "Secure credential storage in Rust with OS keyring"
description: "Store secrets securely in a Rust desktop app using macOS Keychain, Windows Credential Manager, and Linux Secret Service with the keyring crate."
date: 2026-06-05
tags: [Rust, security, tutorial]
draft: false
---

Every desktop app that connects to a service needs to store credentials. API keys, OAuth refresh tokens, database passwords. The first instinct is usually to write them to a config file or a local SQLite database. That works. It also leaves every secret your app handles sitting on disk in plaintext.

This tutorial walks through using the OS credential store from a Rust desktop app. By the end, you'll have a module that writes secrets to macOS Keychain, Windows Credential Manager, or Linux Secret Service, and you'll understand why the alternatives fall short.

## Why config files and databases are the wrong place

A JSON or TOML config file containing `"api_key": "sk-..."` is readable by any process with the same user permissions. That includes browser extensions, shell scripts, build tools, and whatever npm package someone installed last week. SQLite is no better: the default format has no encryption, and backup tools happily copy `.db` files to external drives and cloud storage.

I've seen crash reporters attach config files to bug tickets. I've seen CI pipelines log environment variables that contained file paths, which led someone to cat the file in a debug step. The attack surface adds up faster than you'd expect.

The OS credential store solves this because:

1. Credentials are encrypted at rest using keys tied to the user's login
2. Your app never handles encryption keys directly
3. Access can be scoped to specific applications (especially on macOS with code signing)
4. The user already trusts this system for passwords, certificates, and SSH keys

## How each platform handles it

Before writing code, it helps to know what happens under the hood.

macOS Keychain Services stores items in the user's login keychain. Items can be restricted to a specific code-signed application, so even another process running as the same user cannot read them without the keychain access prompt. The Security.framework provides the C API.

Windows Credential Manager uses DPAPI (Data Protection API) to encrypt credentials. They're stored in the user's profile directory but encrypted with a key derived from the user's Windows credentials. Generic credentials are the type most desktop apps use.

Linux Secret Service exposes a D-Bus interface backed by GNOME Keyring or KDE Wallet. Encryption uses the user's login credentials through PAM integration. This requires an active D-Bus session, which means headless environments and minimal window managers might not have it available.

The important shared property: none of these stores write plaintext to disk. The OS kernel or a privileged daemon handles the cryptography. Your app makes a request and gets back the value.

## Setting up the keyring crate

The `keyring` crate (version 3.x) wraps all three platforms behind one API. Add it to your `Cargo.toml`:

```toml
[dependencies]
keyring = "3"
```

On macOS, I recommend enabling the `apple-native` feature. This uses the Security.framework directly instead of shelling out to the `security` CLI tool, which is faster and avoids parsing command output:

```toml
[dependencies]
keyring = { version = "3", features = ["apple-native"] }
```

The crate selects the correct backend at compile time based on your target OS. No runtime detection needed.

## Basic operations: set, get, delete

The API centers around an `Entry`, identified by a service name and a username (or key):

```rust
use keyring::Entry;

fn main() -> keyring::Result<()> {
    // Create an entry in the OS credential store
    let entry = Entry::new("my-app", "api-token")?;

    // Store a secret
    entry.set_password("sk-secret-value-here")?;

    // Retrieve it later
    let password = entry.get_password()?;
    assert_eq!(password, "sk-secret-value-here");

    // Delete when no longer needed (e.g., user signs out)
    entry.delete_credential()?;

    Ok(())
}
```

Three things to notice here. First, `Entry::new` can fail if the platform has no credential store. This is the correct behavior: you want to know early that secure storage is unavailable, not discover it later when `get_password` returns garbage from a fallback file.

Second, `get_password` returns `Err(NoEntry)` if the credential doesn't exist, which is different from a platform error. You need to handle both cases.

Third, the service name acts as a namespace. Use something unique to your app, like a reverse-domain identifier (`com.example.myapp`). This prevents collisions if the user runs multiple apps that use the keyring.

## Handling the NoEntry case

The distinction between "not found" and "something went wrong" matters in practice. When a user hasn't configured an API key yet, that's normal. When the keyring daemon crashes mid-read, that's an error. Handle them separately:

```rust
use keyring::{Entry, Error};

fn get_api_token() -> Option<String> {
    let entry = Entry::new("my-app", "api-token").ok()?;
    match entry.get_password() {
        Ok(token) => Some(token),
        Err(Error::NoEntry) => {
            // User hasn't stored a token yet. Normal state.
            None
        }
        Err(err) => {
            tracing::error!("Failed to read from keyring: {err}");
            None
        }
    }
}
```

The function returns `None` for both the missing-entry case and the error case, but it logs the error so you can tell them apart during debugging. In a real app, you might surface the error in the UI so the user knows their keyring is having problems.

## Wrapping it in a service module

For a production app, you want to wrap the raw keyring calls in a module that handles initialization, error types, and availability checks. Here's a pattern that works well:

```rust
use keyring::Entry;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("keyring unavailable: {0}")]
    Unavailable(String),
    #[error("secret not found for {service}/{key}")]
    NotFound { service: String, key: String },
    #[error("failed to write secret: {0}")]
    WriteFailed(#[source] keyring::Error),
    #[error("failed to read secret: {0}")]
    ReadFailed(#[source] keyring::Error),
}

pub struct SecureStore {
    service: String,
    available: bool,
}

impl SecureStore {
    pub fn new(service: &str) -> Self {
        // Probe the keyring by attempting to create a test entry
        let available = Entry::new(service, "availability-check").is_ok();
        if !available {
            tracing::warn!(
                "OS keyring is not available. Secret storage disabled."
            );
        }
        Self {
            service: service.to_string(),
            available,
        }
    }

    pub fn is_available(&self) -> bool {
        self.available
    }

    pub fn set(
        &self,
        key: &str,
        value: &str,
    ) -> Result<(), StorageError> {
        if !self.available {
            return Err(StorageError::Unavailable(
                "keyring not initialized".into(),
            ));
        }
        let entry = Entry::new(&self.service, key)
            .map_err(StorageError::WriteFailed)?;
        entry.set_password(value)
            .map_err(StorageError::WriteFailed)?;
        tracing::info!(key, "secret written to keyring");
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        if !self.available {
            return Err(StorageError::Unavailable(
                "keyring not initialized".into(),
            ));
        }
        let entry = Entry::new(&self.service, key)
            .map_err(StorageError::ReadFailed)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(StorageError::ReadFailed(err)),
        }
    }

    pub fn delete(&self, key: &str) -> Result<(), StorageError> {
        if !self.available {
            return Err(StorageError::Unavailable(
                "keyring not initialized".into(),
            ));
        }
        let entry = Entry::new(&self.service, key)
            .map_err(|e| StorageError::WriteFailed(e))?;
        entry.delete_credential()
            .map_err(StorageError::WriteFailed)?;
        tracing::info!(key, "secret deleted from keyring");
        Ok(())
    }
}
```

The `new` function probes the keyring immediately. If the OS has no credential store (headless Linux, CI environment), `available` is `false` and every subsequent call returns `Unavailable`. The app can check `is_available()` once at startup and adjust its behavior. This is better than failing on the first `set` call when the user is mid-login flow.

## The reference pattern: what goes in the database

One pattern worth adopting: keep references in your database, keep values in the keyring.

Your SQLite table holds:

```
key: "github-integration-id"
value: "account_42"
```

The keyring holds:

```
service: "my-app"
username: "github-token-account-42"
password: "ghp_xxxxxxxxxxxx"
```

The database tells you which integrations exist. The keyring holds the credentials to use them. If someone copies the database file, they get a list of IDs. No tokens. No passwords. Nothing they can authenticate with.

This also simplifies backups. You can back up the database to cloud storage or a USB drive without worrying about leaking secrets. Each machine's keyring holds its own credentials. When you import a config on a new machine, the app prompts for API keys that the keyring doesn't have yet.

Read the [secure storage docs](/docs/secure-storage/) for more detail on how this works in a full application.

## What about Linux without a keyring?

Headless Linux servers, CI runners, and minimal window managers sometimes have no D-Bus session and no keyring daemon. The `keyring` crate handles this by returning an error from `Entry::new`.

Your app should handle this explicitly. Disable features that require credentials. Show a clear message in the UI. Do not silently fall back to storing secrets in a file. Silent fallback trains users to ignore security boundaries and creates a false sense of safety. If the keyring is unavailable, the right answer is to not store the secret.

```rust
let store = SecureStore::new("my-app");
if !store.is_available() {
    eprintln!(
        "OS keyring not found. Features requiring credentials will be disabled."
    );
}
```

Some apps solve this by bundling a software keyring like `gnome-keyring-daemon` or using the `keyring` crate's built-in mock store for testing. That works for development. For production, I'd rather tell the user their environment is misconfigured than silently weaken their security.

## Testing without a real keyring

The `keyring` crate provides a mock store for testing. It lives in memory and never touches the OS keyring:

```rust
use keyring::{use_named_store, release_store};
use keyring::Entry;

#[test]
fn test_store_and_retrieve() {
    use_named_store("sample").expect("mock store should init");

    let entry = Entry::new("test-app", "test-key").unwrap();
    entry.set_password("test-value").unwrap();
    assert_eq!(entry.get_password().unwrap(), "test-value");
    entry.delete_credential().unwrap();

    release_store();
}
```

Call `use_named_store("sample")` before any keyring operations in your test, and `release_store()` after. This replaces the OS backend with an in-memory store for the duration of the test. No Keychain prompts, no D-Bus dependency, no credential manager dialogs.

See the [form validation tutorial](/blog/form-validation-rust-tutorial/) for a similar approach to testing with external dependencies.

## Migrating from plaintext storage

If your app currently stores secrets in a config file or database, migration is straightforward. On startup, check if the old plaintext value exists. If it does, write it to the keyring and delete the plaintext copy:

```rust
fn migrate_from_config(store: &SecureStore, config: &mut Config) {
    if let Some(token) = config.api_token.take() {
        match store.set("api-token", &token) {
            Ok(()) => {
                config.api_token = None;
                config.save().expect("config save should work");
                tracing::info!("Migrated API token from config to keyring");
            }
            Err(err) => {
                tracing::error!("Keyring migration failed: {err}");
                // Put it back. Better plaintext than losing the token.
                config.api_token = Some(token);
            }
        }
    }
}
```

The important detail: if the keyring write fails, put the value back. Losing a user's API token because the migration failed is worse than leaving it in plaintext. Log the failure. Show a warning. Let the user retry later.

## Why this matters

Config files with API keys are a credential dump on disk. Backups copy them. Crash reporters attach them. CI logs expose them. The OS credential store exists specifically to prevent this. It encrypts at rest, scopes access, and handles key derivation without your app knowing the details.

For a working implementation of everything covered here, check out [gpui-starter's secure storage module](https://github.com/freeoxide/gpui-starter) on GitHub. It wraps the `keyring` crate with availability probing, typed errors, and the reference-in-database pattern from earlier. The [getting started guide](/docs/getting-started/) walks through setting up the full project.
