---
title: "Cross-platform credential storage in Rust with keyring"
description: "Learn how to use the Rust keyring crate for secure, cross-platform credential storage in macOS Keychain, Windows Credential Manager, and Linux Secret Service."
date: 2026-06-05
tags: [Rust, keyring, security]
draft: false
---

Storing API keys, OAuth tokens, and database passwords is one of those problems every desktop app runs into. You need to persist credentials between sessions, but dumping them in a config file is asking for trouble. The `keyring` crate in Rust gives you a single API that talks to the native credential store on macOS, Windows, and Linux. This post walks through how it works, where it falls short, and how we use it in a real app.

## Why rolling your own encryption is a trap

You could encrypt secrets with AES-GCM and store the ciphertext in SQLite. People do this. The problem is key management. Where does the encryption key live? If it is derived from a hardcoded value, anyone who reads your source can decrypt the secrets. If you prompt the user for a master password on every launch, you have a worse UX than a password manager. If you store the key in a file, you have moved the problem one level down without solving it.

Operating systems already solved this. macOS has Keychain Services, which encrypts items using keys derived from the user's login password. Windows has Credential Manager with DPAPI encryption tied to the user's Windows profile. Linux has the Secret Service API, backed by GNOME Keyring or KDE Wallet, which encrypts with the user's login credentials through PAM.

The `keyring` crate wraps all three of these behind one API. Your code does not need to know which OS it is running on.

## Setting up the keyring crate

Add it to your `Cargo.toml`:

```toml
[dependencies]
keyring = "3"
```

That pulls in default features including native backends for your platform. Feature flags like `apple-native`, `windows-native`, `linux-native`, and `sync-secret-service` control which backends get compiled. The default set picks the right ones for your build target.

## Basic usage: create, read, delete

The core abstraction is `Entry`. You create one by specifying a service name and a username (or key name). The service name is typically your app's bundle identifier or reverse-domain name, like `com.example.myapp`.

```rust
use keyring::Entry;

fn main() -> keyring::Result<()> {
    let entry = Entry::new("com.example.myapp", "api-token")?;

    // Store a credential
    entry.set_password("sk-live-abc123xyz")?;

    // Retrieve it later
    let token = entry.get_password()?;
    assert_eq!(token, "sk-live-abc123xyz");

    // Remove it when no longer needed
    entry.delete_credential()?;

    Ok(())
}
```

Three methods cover the entire lifecycle. `set_password` creates or overwrites. `get_password` reads. `delete_credential` removes. If you call `get_password` on an entry that does not exist, you get `Err(keyring::Error::NoEntry)`, which is the one error variant you will see most often.

The `Entry::new` call does not write anything to the credential store. It just constructs a handle. The actual OS store is only contacted when you call `set_password`, `get_password`, or `delete_credential`.

## What happens on each platform

Here is what `keyring` does under the hood, because you should know where your secrets live.

On macOS, keyring calls Keychain Services via the `security` framework. Credentials are stored as generic password items in the user's login keychain. Access can be restricted to code-signed applications. Items survive across reboots and are synced to iCloud Keychain if the user has that enabled (which you may or may not want).

On Windows, it uses the Windows Credential Manager via `advapi32`. Credentials are encrypted with DPAPI, which ties decryption to the user's Windows account. They persist in the user's profile directory.

On Linux, the default is the kernel keyutils backend, which stores credentials in the user's session keyring. This means secrets vanish on logout or reboot. If you want persistent storage, you need to activate the Secret Service backend instead:

```rust
use keyring::{use_native_store, release_store, Entry};

fn main() -> keyring::Result<()> {
    // true = use Secret Service (GNOME Keyring / KDE Wallet) instead of keyutils
    use_native_store(true)?;

    let entry = Entry::new("com.example.myapp", "api-token")?;
    entry.set_password("sk-live-abc123xyz")?;

    let pw = entry.get_password()?;
    println!("Token: {pw}");

    entry.delete_credential()?;
    release_store();
    Ok(())
}
```

The Secret Service backend requires a D-Bus session and a running keyring daemon. On headless servers or minimal Linux installs, this may not be available. That is a real limitation you need to account for.

## Error handling in practice

The `keyring::Error` enum has variants for the common cases you will hit:

```rust
match entry.get_password() {
    Ok(password) => { /* use it */ }
    Err(keyring::Error::NoEntry) => {
        // The credential does not exist yet.
        // First launch, or user cleared their keychain.
    }
    Err(keyring::Error::Invalid(key, msg)) => {
        // Bad service name or username.
        eprintln!("Invalid entry '{key}': {msg}");
    }
    Err(err) => {
        // Platform-specific failure: keychain locked, D-Bus unavailable, etc.
        eprintln!("Credential store error: {err}");
    }
}
```

The two you care about most are `NoEntry` (expected, handle gracefully) and the catch-all platform errors (keychain locked, Secret Service not running, permission denied). I recommend wrapping `keyring::Error` in your own error type so callers do not need to depend on keyring directly.

## Wrapping keyring in a service layer

In a larger app, you want to decouple your business logic from the keyring API. Here is the pattern I use. Define your own error type:

```rust
#[derive(Debug, thiserror::Error)]
pub enum SecureStorageError {
    #[error("entry creation failed for '{service}/{key}': {source}")]
    EntryCreation {
        service: String,
        key: String,
        source: keyring::Error,
    },
    #[error("failed to set secret for '{service}/{key}': {source}")]
    SetFailed {
        service: String,
        key: String,
        source: keyring::Error,
    },
    #[error("failed to get secret for '{service}/{key}': {source}")]
    GetFailed {
        service: String,
        key: String,
        source: keyring::Error,
    },
}
```

Then expose three functions that map errors and handle the `NoEntry` case:

```rust
pub fn set_secret(
    service: &str,
    key: &str,
    value: &str,
) -> Result<(), SecureStorageError> {
    let entry = keyring::Entry::new(service, key).map_err(|err| {
        SecureStorageError::EntryCreation {
            service: service.into(),
            key: key.into(),
            source: err,
        }
    })?;

    entry.set_password(value).map_err(|err| {
        SecureStorageError::SetFailed {
            service: service.into(),
            key: key.into(),
            source: err,
        }
    })?;

    Ok(())
}

pub fn get_secret(
    service: &str,
    key: &str,
) -> Result<Option<String>, SecureStorageError> {
    let entry = keyring::Entry::new(service, key).map_err(|err| {
        SecureStorageError::EntryCreation {
            service: service.into(),
            key: key.into(),
            source: err,
        }
    })?;

    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(SecureStorageError::GetFailed {
            service: service.into(),
            key: key.into(),
            source: err,
        }),
    }
}
```

Returning `Ok(None)` for missing entries instead of an error makes the calling code cleaner. On first launch, no credential exists yet, and that is fine. The caller checks `if let Some(token) = get_secret(...)?` and starts the OAuth flow if needed.

## Checking availability at startup

Not every system has a working credential store. Headless Linux machines, CI environments, and sandboxed test runners may not have D-Bus or a keyring daemon. You should check early and degrade gracefully rather than crashing on the first `set_password` call.

```rust
pub fn is_available() -> bool {
    keyring::Entry::new("my-app", "availability-check").is_ok()
}
```

This does not write anything. It just verifies that the keyring backend can create an `Entry` handle. If this returns `false`, fall back to an alternative storage mechanism or show a notice to the user. Store the result in your app's global state so you only check once.

## The tradeoffs you should know about

The `keyring` crate works for most desktop apps, but it has real limitations.

Linux persistence requires a daemon. The default keyutils backend stores credentials in memory. They vanish on reboot. For persistent storage, you need Secret Service, which requires GNOME Keyring or KDE Wallet to be running. On servers or minimal installs, this is often not available. If you need to support headless Linux, consider [storing secrets in an encrypted SQLite database](/blog/sqlite-rust-desktop-tutorial/) as a fallback.

There is no sync between machines. iCloud Keychain sync on macOS can carry credentials to other Macs. On Windows and Linux, credentials stay local. If your app needs to sync tokens across devices, you need your own backend for that.

Watch out for credential size limits. Keychain Services on macOS has a practical limit around 20KB per item. Windows Credential Manager is similar. For large secrets (SSH keys, certificates), store a reference or encryption key in the keyring and keep the actual data in a file.

Thread safety is worth noting. `Entry` is `Send` but not `Sync`. You can move entries between threads but cannot share one across threads concurrently. For concurrent access, create separate `Entry` instances on each thread.

## When keyring is not the right fit

If you are building a CLI tool that runs in CI, the keyring is probably not available. If you need to share credentials between multiple machines, the OS keyring does not help. If you are storing megabytes of data, you will hit size limits. In those cases, use an encrypted file with a key derived from an environment variable, or read about [form validation and secure input handling](/blog/form-validation-rust-tutorial/) to build a master-password flow.

For the common case of a desktop app that needs to remember an API key or OAuth refresh token between launches, `keyring` is the best option in the Rust ecosystem. It is maintained by the open-source cooperative, has been around since 2017, and handles the platform differences so you do not have to.

If you want to see this integrated into a full desktop application, check out [gpui-starter](https://github.com/freeoxide/gpui-starter) which uses keyring for secure credential storage alongside SQLite persistence, i18n, and a theme system. The [getting started guide](/docs/getting-started/) walks through the full setup.
