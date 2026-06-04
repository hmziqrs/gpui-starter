---
title: "Crash reporting for Rust desktop applications"
description: "Setting up crash report generation in Rust desktop apps: panic hooks, minidumps, error reporting, and diagnostics for post-mortem debugging."
date: 2026-06-05
tags: [Rust, crash, debugging]
draft: false
---

Your app crashed. The user saw a window vanish. No dialog, no "report problem" prompt, nothing. You have no idea why. If you are building a desktop application in Rust, this scenario is going to happen, and you need a plan for it before it does.

Crash reporting in Rust desktop apps differs from web or server environments. There is no cloud error aggregator automatically collecting your stack traces. You are running on someone else's machine, inside their OS, with their particular hardware and driver combination. The crash data has to be collected locally, stored safely, and sent back to you when possible.

This post covers the practical pieces: intercepting panics, capturing useful diagnostic data, writing it to disk, and getting it off the user's machine.

## Why panics are not enough

Rust's panic handler prints a message to stderr and unwinds the thread (or aborts, depending on your panic strategy). That is fine during development. In production, stderr goes nowhere visible. The user never sees it. You never see it.

You need to intercept the panic before the default handler runs, capture the context that matters, and persist it somewhere your app can find on the next launch.

## Installing a custom panic hook

The standard library gives you `std::panic::take_hook()` and `std::panic::set_hook()`. The pattern is straightforward: save the previous hook, install your own, and chain to the previous one at the end.

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static LAST_PANIC_SUMMARY: OnceLock<Mutex<Option<String>>> = OnceLock::new();

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let summary = info.to_string();

        // Store the summary for the app to read on the next frame.
        let slot = LAST_PANIC_SUMMARY.get_or_init(|| Mutex::new(None));
        if let Ok(mut value) = slot.lock() {
            *value = Some(summary.clone());
        }

        // Write a crash report to disk (covered below).
        write_crash_report_from_panic(&summary);

        // Chain to the default handler so you still get stderr output.
        previous(info);
    }));
}
```

Call this early in `main()`, before any other code runs. If your app uses `panic = "abort"` in Cargo.toml, the panic hook still fires, but `set_hook` cannot prevent the process from terminating after it returns. The hook is your one chance to write something to disk.

### A note on panic vs. abort

With `panic = "unwind"` (the default), a panic in a spawned thread kills that thread but leaves the rest of the process alive. You can recover from this in a UI app by catching the thread join and showing an error view. With `panic = "abort"`, the entire process dies. The panic hook still runs, but you have no opportunity for in-process recovery.

For desktop UI apps, I prefer `panic = "unwind"` specifically because it lets the app survive a render thread panic and show an error boundary instead of vanishing.

## Capturing useful crash data

A panic message alone is rarely enough to diagnose a bug. You want the backtrace, the app version, the OS and architecture, and ideally some recent error context.

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrashReport {
    pub id: String,
    pub panic_message: String,
    pub backtrace: String,
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub timestamp: String,
    pub recent_errors: Vec<String>,
}

impl CrashReport {
    pub fn new(panic_message: String, recent_errors: Vec<String>) -> Self {
        let backtrace = std::backtrace::Backtrace::capture();
        let bt_string = match backtrace.status() {
            std::backtrace::BacktraceStatus::Captured => backtrace.to_string(),
            _ => String::new(),
        };

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            panic_message,
            backtrace: bt_string,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            recent_errors,
        }
    }
}
```

The `std::backtrace::Backtrace::capture()` call works without any external crates, but it requires `RUST_BACKTRACE=1` to be set at runtime. You can set this programmatically in main:

```rust
fn main() {
    if std::env::var("RUST_BACKTRACE").is_err() {
        std::env::set_var("RUST_BACKTRACE", "1");
    }
    // ...
}
```

### Tracking recent errors before a crash

A panic does not happen in isolation. There are usually warning signs: failed network requests, storage errors, configuration issues. If you track these in a ring buffer, you can attach them to the crash report.

```rust
static RECENT_ERRORS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

pub fn track_recent_error(msg: String) {
    let slot = RECENT_ERRORS.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut guard) = slot.lock() {
        guard.push(msg);
        if guard.len() > 20 {
            guard.remove(0);
        }
    }
}
```

Call `track_recent_error` from your error surface or error boundary every time a non-fatal error occurs. When the panic handler fires, it reads the buffer and includes it in the crash report. This gives you the chain of events leading up to the crash, not just the final panic message.

## Writing crash reports to disk

The panic hook runs in a constrained environment. You cannot use async I/O. You should not allocate heavily. `std::fs` is your only real option.

```rust
use std::path::Path;

pub fn write_crash_report(report: &CrashReport, data_dir: &Path) -> std::io::Result<()> {
    let reports_dir = data_dir.join("crash_reports");
    std::fs::create_dir_all(&reports_dir)?;

    let file_path = reports_dir.join(format!("{}.json", report.id));
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(&file_path, json)?;
    Ok(())
}
```

The `data_dir` path should be resolved once during startup and stored in a static so the panic hook can access it without GPUI context. Use `OnceLock<PathBuf>` for this.

Why JSON and not a binary format? Because you will need to read these reports in multiple contexts: on the next app launch to show "you experienced a crash" UI, in your upload endpoint, and potentially during manual debugging. JSON is the lowest common denominator.

## Detecting crashes on next launch

After a crash, the app process is gone. On the next launch, you need to know a crash happened. Two common approaches: scan the crash report directory for files, or use a crash marker file.

A crash marker is a file you write at startup and delete on clean shutdown. If the file exists on the next launch, the previous run crashed.

```rust
fn crash_marker_path() -> PathBuf {
    std::env::temp_dir().join("my-app.crash-marker")
}

pub fn write_crash_marker() {
    let path = crash_marker_path();
    let pid = std::process::id();
    let timestamp = chrono::Utc::now().to_rfc3339();
    let _ = std::fs::write(&path, format!("pid={pid}\nstarted_at={timestamp}\n"));
}

pub fn check_previous_crash() -> Option<String> {
    let path = crash_marker_path();
    if path.exists() {
        std::fs::read_to_string(&path).ok()
    } else {
        None
    }
}

pub fn remove_crash_marker() {
    let path = crash_marker_path();
    let _ = std::fs::remove_file(&path);
}
```

The sequence is: `write_crash_marker()` early in main, `remove_crash_marker()` during clean shutdown. On next launch, `check_previous_crash()` tells you whether the previous session ended abnormally.

This approach has one limitation: if the machine loses power, the marker file might still be present from a clean run. The PID and timestamp help you distinguish stale markers from real crashes.

## Uploading crash reports

Once you have crash report files on disk, you need to get them to your servers. This should happen asynchronously on the next app launch, not during the panic handler.

```rust
pub fn upload_pending_reports(cx: &mut App, endpoint: &str) {
    let reports = detect_pending_reports(&data_dir);
    if reports.is_empty() {
        return;
    }

    cx.spawn(async move |cx| {
        for report in &reports {
            let body = serde_json::to_string(report)?;
            let resp = http_client
                .post(endpoint)
                .header("Content-Type", "application/json")
                .body(body)
                .send()
                .await;

            if let Ok(resp) = resp && resp.status().is_success() {
                // Mark report as uploaded so you don't re-send it.
                mark_uploaded(&report.id);
            }
        }
    }).detach();
}
```

Rate-limit uploads and batch them. Do not upload 50 crash reports at once. A reasonable strategy is to upload the 5 most recent reports per launch and leave the rest for subsequent launches.

### Privacy and crash reports

Crash reports contain file paths, thread names, and sometimes user data in panic messages. Before uploading anything, consider:

- Redacting absolute file paths (replace `/Users/alice/...` with `$HOME/...`)
- Stripping environment variables from backtraces
- Giving users an opt-out setting
- Publishing a privacy policy explaining what you collect

For apps distributed through app stores, you may also need to disclose crash data collection in your privacy manifest.

## When to use minidumps instead

Everything above handles Rust panics. But what about segfaults, stack overflows, and other signals that bypass the panic handler? For those, you need a minidump.

The `minidump` and `crash-handler` crates from the rust-minidump project give you out-of-process crash handling. This is what Firefox and Chrome use. The overhead is significant: you need a separate crash reporter process, symbol files for every build, and a minidump stackwalker to make the dumps readable.

For most desktop applications, panic-only reporting covers 95% of crashes. Add minidump support if you are doing FFI with C libraries, interfacing with system APIs directly, or seeing unexplained process exits with no panic output.

## Testing your crash pipeline

You cannot fully test crash reporting in unit tests. The panic hook is process-global, and real crashes interact with the OS in ways that are hard to mock. But you can test the individual pieces:

- Write a crash report, read it back, verify all fields round-trip
- Confirm that malformed JSON files are skipped during detection
- Verify the crash marker lifecycle (write, check, remove)
- Test that `track_recent_error` caps the buffer at the expected size

I cover more testing strategies in [testing GPUI applications](/blog/testing-gpui-applications/) if you want to go deeper.

## Putting it together

The full pipeline in a Rust desktop app looks like this:

1. Install a custom panic hook at the very start of `main()`
2. Track non-fatal errors in a ring buffer throughout the app's lifetime
3. When a panic occurs, capture the backtrace, attach recent errors, and write a JSON report to disk
4. Write a crash marker file at startup, delete it on clean exit
5. On next launch, check for the crash marker and scan for pending reports
6. Upload pending reports to your server, marking each as uploaded on success
7. Show the user a "previous session crashed" notification with a link to details

This is the approach used in [gpui-starter](https://github.com/hmziqrs/gpui-boilerplate), which includes the complete crash reporting pipeline described here plus an error boundary that catches render panics and shows a recovery UI instead of killing the window. Check out the [getting started guide](/docs/getting-started/) if you want to see the full implementation.
