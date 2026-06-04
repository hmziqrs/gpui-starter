---
title: "Security best practices for Rust desktop applications"
description: "Securing Rust desktop apps: credential storage, update signing, input validation, and dependency auditing."
date: 2026-06-05
tags: [Rust, security, guide]
draft: false
---

Desktop applications have a different threat model than web apps. Your code runs on someone else's machine. They can inspect binaries, modify local files, and intercept network traffic. Rust gives you memory safety for free, but that only covers one class of bugs. The rest is on you.

I have spent the last year building [gpui-starter](https://github.com/hmziqrs/gpui-boilerplate), a Rust desktop boilerplate using GPUI. Security came up repeatedly: how to store API keys, how to ship updates without getting supply-chain attacked, how to validate form input, and how to audit dependencies. Here is what I learned, with code.

## Store credentials in the OS keychain, not in files

The first mistake I see in Rust desktop apps is writing API tokens to a config file in plaintext. Sometimes it is a JSON file in `~/.config`. Sometimes it is a TOML file next to the binary. Either way, any process running as the same user can read it.

macOS has Keychain. Windows has Credential Manager. Linux has Secret Service (via GNOME Keyring or KDE Wallet). Use them.

The `keyring` crate provides a cross-platform wrapper:

```rust
use keyring::Entry;

fn store_api_token(service: &str, username: &str, token: &str) -> Result<(), keyring::Error> {
    let entry = Entry::new(service, username)?;
    entry.set_password(token)
}

fn get_api_token(service: &str, username: &str) -> Result<String, keyring::Error> {
    let entry = Entry::new(service, username)?;
    entry.get_password()
}
```

The tradeoff is that the keyring crate adds a native dependency on each platform. On Linux, you also need to handle the case where no secret service is running (headless servers, some WSL setups). I fall back to encrypting with a machine-specific key derived from system properties, then storing the ciphertext in a file. It is not as strong as the OS keychain, but it is better than plaintext.

For the encryption fallback, I use `aes-gcm` with a key derived from a machine fingerprint:

```rust
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aes_gcm::aead::Aead;
use sha2::{Sha256, Digest};

fn machine_key() -> [u8; 32] {
    let mut hasher = Sha256::new();
    // Combine hostname + username + platform as a machine fingerprint
    hasher.update(hostname().unwrap_or_default().as_bytes());
    hasher.update(whoami().unwrap_or_default().as_bytes());
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

fn encrypt_for_local(data: &[u8]) -> Result<Vec<u8>, aes_gcm::Error> {
    let key = machine_key();
    let cipher = Aes256Gcm::new_from_slice(&key)?;
    let nonce = Nonce::from_slice(b"unique nonce"); // In production, use a random nonce
    cipher.encrypt(nonce, data)
}
```

The key point: never store secrets in a format that another app on the same machine can trivially read.

## Sign your updates

Auto-updating is a major attack surface. If an attacker can trick your app into downloading and running a malicious binary, game over. This is how many supply chain attacks work.

You need cryptographic signatures on your update manifests. The app verifies the signature before it installs anything.

I use Ed25519 for signing update manifests. The private key lives on the build server, never in the app. The public key is embedded in the binary at compile time:

```rust
/// Public key embedded in the binary for update verification.
/// Corresponding private key is held by the CI pipeline.
const UPDATE_PUBLIC_KEY: &[u8; 32] = include_bytes!("../update_pub.key");

fn verify_update_signature(manifest: &[u8], signature: &[u8]) -> bool {
    use ed25519_dalek::{VerifyingKey, Signature, Verifier};

    let VerifyingKey::from_bytes(UPDATE_PUBLIC_KEY)
        .ok()
        .and_then(|pk| {
            let sig = Signature::from_bytes(signature.try_into().ok()?);
            pk.verify(manifest, &sig).ok()
        })
        .is_some()
}
```

The update manifest is a JSON file containing the version number, download URL, file hash (SHA-256), and the Ed25519 signature over the hash. The app checks:

1. The signature validates against the embedded public key.
2. The SHA-256 of the downloaded binary matches the hash in the manifest.
3. The version in the manifest is newer than the current version.

Skip any of these checks and you are shipping an exploitable auto-updater. The [Electron incident of 2024](https://www.electronjs.org/docs/latest/tutorial/security) was a reminder of what happens when update verification is incomplete.

## Validate all input at the boundary

Rust's type system catches a lot of bugs at compile time. But it does not stop a user from pasting a 10MB string into a text field, or submitting a form with HTML in a field that gets rendered later.

Input validation in desktop apps falls into two categories: preventing crashes (resource exhaustion), and preventing injection (XSS, SQL injection, command injection).

For forms, I use `koruma` with `gpui-form` to define validation rules declaratively:

```rust
use koruma::Validate;

#[derive(Validate)]
struct LoginForm {
    #[koruma(length(min = 3, max = 32))]
    username: String,

    #[koruma(length(min = 8, max = 128))]
    password: String,
}
```

The [form validation tutorial](/blog/form-validation-rust-tutorial/) covers this in more detail. The point is that validation happens at the boundary, right when user input enters the system, before it touches any business logic.

For SQL queries, I use parameterized statements exclusively. Rust makes this straightforward with `rusqlite`:

```rust
// Correct: parameterized query
let mut stmt = conn.prepare("SELECT * FROM users WHERE id = ?1")?;
let rows = stmt.query_map(params![user_id], |row| {
    Ok(User { id: row.get(0)?, name: row.get(1)? })
})?;

// Never do this:
// let query = format!("SELECT * FROM users WHERE id = {}", user_id);
```

String formatting into SQL queries is the kind of thing that feels harmless in a desktop app ("the database is local, who would inject it?") until someone figures out they can corrupt the local store through a crafted input file or deep link.

## Audit your dependency tree

Rust's crate ecosystem is excellent, but every dependency you add is code you did not write running with the same privileges as your code. The `regex` crate had a supply chain attempt in 2024. The `xz` backdoor in 2025 affected Linux distributions. Supply chain attacks are real.

Run `cargo audit` regularly. I have it in CI on every pull request:

```bash
cargo install cargo-audit
cargo audit
```

This checks every dependency against the RustSec advisory database. It catches known vulnerabilities in crate versions you are using.

Go further with `cargo deny`:

```toml
# deny.toml
[advisories]
vulnerability = "deny"
unmaintained = "warn"

[licenses]
allow = ["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib"]
unlicensed = "deny"

[bans]
multiple-versions = "warn"
wildcards = "deny"
```

`cargo deny` checks licenses (you do not want to accidentally ship GPL code in a proprietary app), catches duplicate versions of the same crate, and flags wildcard dependency specifications. Run it in CI alongside `cargo audit`.

Keep your dependency count low. Every crate you add is a trust decision. Before adding a crate, check: when was the last commit? How many maintainers does it have? Does it have existing security advisories? A crate with one maintainer and no commits in two years is a liability.

## Handle crashes without leaking sensitive data

When your app panics, the default Rust backtrace can contain file paths, environment variable names, and fragments of local variables. On a desktop app, the crash report might get uploaded to your server or copied into a GitHub issue.

Strip sensitive data from crash reports before they leave the machine:

```rust
use std::panic;

fn setup_crash_handler() {
    panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::capture();
        let sanitized = sanitize_backtrace(&backtrace.to_string());

        // Store locally for the user to review before sending
        let report = CrashReport {
            message: info.to_string(),
            backtrace: sanitized,
            timestamp: chrono::Utc::now(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        };

        if let Err(e) = save_crash_report(&report) {
            eprintln!("Failed to save crash report: {e}");
        }
    }));
}

fn sanitize_backtrace(bt: &str) -> String {
    bt.lines()
        .map(|line| {
            // Redact home directory paths
            let home = dirs::home_dir().unwrap_or_default();
            let home_str = home.to_string_lossy();
            line.replace(&*home_str, "$HOME")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
```

This approach stores crash data locally and lets the user review it before any upload happens. No silent telemetry. No sending stack traces that include `/Users/realname/projects/` in the path.

The [architecture guide](/docs/architecture/) covers how gpui-starter structures crash reporting as a service that the user opts into during first-run setup.

## Use single-instance to prevent local attacks

Most desktop apps should run as a single instance. If your app stores data locally (SQLite, keychain entries, config files), running two copies simultaneously can cause data corruption or race conditions. It also opens the door to a local attack: someone launches a second copy of your app with modified environment variables or config file paths.

Use a file lock or named pipe to enforce single-instance:

```rust
use fs4::FileExt;
use std::fs::File;

fn acquire_single_instance_lock(app_name: &str) -> Option<File> {
    let lock_path = dirs::data_local_dir()?
        .join(app_name)
        .join("instance.lock");

    let file = File::create(&lock_path).ok()?;

    // Try to acquire an exclusive lock (non-blocking)
    match file.try_lock_exclusive() {
        Ok(()) => Some(file),
        Err(_) => {
            eprintln!("Another instance is already running.");
            None
        }
    }
}
```

The lock file is released automatically when the process exits, even on crash. If the lock acquisition fails, show a dialog and focus the existing window instead of launching a second copy.

## Compile with hardening flags

The Rust compiler produces reasonably secure binaries by default, but there are a few flags worth enabling for a desktop release build:

```toml
# Cargo.toml release profile
[profile.release]
opt-level = 3
lto = true
strip = true
panic = "abort"
codegen-units = 1
```

`strip = true` removes debug symbols from the binary, making reverse engineering harder. `panic = "abort"` prevents unwinding, which eliminates an entire class of memory safety issues that could theoretically arise from panic handling in unsafe code. `lto = true` with `codegen-units = 1` produces a smaller, more optimized binary and enables better cross-crate optimization.

On macOS, also set `MACOSX_DEPLOYMENT_TARGET` to a recent SDK version to ensure you get the latest security mitigations from the linker.

## The checklist

Here is what I review before every release of a Rust desktop app:

- Credentials stored in OS keychain (with encrypted fallback)
- Update manifests signed with Ed25519, public key embedded in binary
- All user input validated at the boundary with `koruma`
- All SQL queries parameterized
- `cargo audit` and `cargo deny` passing in CI
- Crash reports sanitized before storage, no automatic upload
- Single-instance lock enforced
- Release profile hardened (strip, LTO, panic abort)

Security is not a feature you bolt on at the end. It is a set of habits you build into the development cycle. Whenever you add a dependency, store user data, or download something from the network, ask: what would happen if this input were hostile?

gpui-starter implements all of these patterns out of the box. If you are building a Rust desktop app with GPUI, the [getting started guide](/docs/getting-started/) walks through the security configuration. The full source is on [GitHub](https://github.com/hmziqrs/gpui-boilerplate).
