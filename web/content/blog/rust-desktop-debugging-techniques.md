---
title: "Debugging techniques for Rust desktop applications"
description: "Practical debugging strategies for Rust desktop apps: logging with tracing, panic hooks, remote diagnostics, frame-time profiling, and the tools that actually help."
date: 2026-06-05
tags: [Rust, debugging, techniques]
draft: false
---

A Rust desktop app crashes on a user's machine. You get a vague email: "it just closed." No stack trace, no reproduction steps, no error message. This is the fundamental debugging problem for desktop software. You are not running the code. The user is, on hardware you have never seen, with an OS version you did not test.

This post covers the debugging techniques I use in Rust desktop applications, from the basics (logging and panic hooks) to the less obvious ones (frame-time profiling, diagnostics bundles, and remote log collection). These are not theoretical recommendations. I have used all of them in production GPUI apps and they have shipped fixes I would not have found otherwise.

## Start with structured logging

If your app prints to stdout during development and ships with no log output, stop. Go install `tracing` now. I wrote about [structured logging with tracing](/blog/integrating-tracing-rust-logging/) in a previous post, but the short version: set up dual output to stdout and rolling files, attach structured fields to every event, and make sure the file appender's `WorkerGuard` stays alive for the entire lifetime of the process.

The point is that desktop app logs need to survive the app. When the user reports a bug, you ask them to find the log file and email it. That log file has to contain enough context to tell you what happened. Structured fields make this queryable:

```rust
tracing::error!(
    target: "app::database",
    path = %db_path.display(),
    error = %err,
    elapsed_ms = elapsed.as_millis() as u64,
    "database migration failed"
);
```

With this in the log file, you can grep for `migration failed` or filter on the `path` field without parsing freeform strings.

### Per-module filtering

Not every module needs the same log level. Your database layer should log at `debug` by default. Your render loop should probably be `warn` unless you are actively chasing a rendering bug. `tracing-subscriber` lets you set per-module filters:

```rust
use tracing_subscriber::EnvFilter;

let filter = EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| {
        EnvFilter::new("app=info,app::render=warn,app::db=debug")
    });
```

This keeps log files manageable in production while still giving you depth where you need it.

## Catch panics before they vanish

Rust's default panic handler prints to stderr and unwinds the thread. In a desktop app, stderr is invisible to the user. The window disappears and they have no idea what happened.

Install a custom panic hook early in `main()`:

```rust
pub fn install_panic_hook(log_dir: &Path) {
    let log_dir = log_dir.to_owned();
    let previous = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        let summary = info.to_string();
        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");

        // Write crash report to disk
        let crash_file = log_dir.join(format!("crash_{timestamp}.log"));
        if let Ok(mut f) = std::fs::File::create(&crash_file) {
            use std::io::Write;
            let _ = writeln!(f, "PANIC at {}", timestamp);
            let _ = writeln!(f, "{summary}");
            let _ = writeln!(f, "\nBacktrace:");
            let _ = writeln!(f, "{}", std::backtrace::Backtrace::capture());
        }

        // Chain to previous handler
        previous(info);
    }));
}
```

Call this before anything else in `main()`. With `panic = "unwind"` (the default), a panic in a spawned thread kills that thread but the rest of the process keeps running. This means you can catch the panic, show an error boundary in the UI, and let the user save their work instead of losing everything. If you use `panic = "abort"`, the hook still fires but you get no chance at recovery after it returns.

For a deeper treatment of crash handling, see [crash reporting for Rust desktop apps](/blog/rust-desktop-crash-reporting/).

## Frame-time profiling for UI performance

Desktop UI apps have a different performance profile than web or CLI tools. The user perceives jank when a frame takes longer than 16ms (for 60fps). The problem is usually not total CPU usage but specific frames that spike.

GPUI gives you frame timing data. You can wrap individual render steps with timing instrumentation:

```rust
#[instrument(skip(cx), fields(step = %step_name))]
fn measure_render_step(step_name: &str, cx: &mut App) {
    let start = std::time::Instant::now();
    // ... render work ...
    let elapsed = start.elapsed().as_micros() as u64;
    if elapsed > 8000 {
        tracing::warn!(
            target: "app::perf",
            step = %step_name,
            elapsed_us = elapsed,
            "slow render step detected"
        );
    }
}
```

This logs a warning whenever a single step takes more than 8ms (half the frame budget at 60fps). Over time you build a picture of which operations are consistently slow and which only spike under specific conditions.

You can also track this per-frame in a circular buffer and expose it as an in-app diagnostics view:

```rust
struct FrameTimes {
    samples: Vec<u64>,  // microseconds
    index: usize,
}

impl FrameTimes {
    fn record(&mut self, frame_us: u64) {
        if self.samples.len() < self.samples.capacity() {
            self.samples.push(frame_us);
        } else {
            self.samples[self.index] = frame_us;
            self.index = (self.index + 1) % self.samples.capacity();
        }
    }

    fn p95(&self) -> u64 {
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        sorted[(sorted.len() as f64 * 0.95) as usize]
    }
}
```

A 120-sample ring buffer gives you the last two seconds of frame data. The p95 value tells you more about perceived performance than the average ever will.

## Build a diagnostics bundle

When a user files a bug report, asking them to find log files, check their OS version, and describe their hardware is a losing game. Instead, build a diagnostics bundle your app can export on demand.

```rust
pub fn generate_diagnostics_bundle() -> Result<String, Box<dyn std::error::Error>> {
    let mut bundle = String::new();

    bundle.push_str("=== System Info ===\n");
    bundle.push_str(&format!("OS: {}\n", std::env::consts::OS));
    bundle.push_str(&format!("Arch: {}\n", std::env::consts::ARCH));
    bundle.push_str(&format!("App version: {}\n", env!("CARGO_PKG_VERSION")));

    bundle.push_str("\n=== Recent Logs ===\n");
    // Read last N lines from the current log file
    if let Ok(logs) = std::fs::read_to_string(recent_log_path()?) {
        let lines: Vec<&str> = logs.lines().rev().take(200).collect();
        for line in lines.into_iter().rev() {
            bundle.push_str(line);
            bundle.push('\n');
        }
    }

    bundle.push_str("\n=== Crash Reports ===\n");
    if let Ok(entries) = std::fs::read_dir(crash_dir()?) {
        for entry in entries.flatten().take(5) {
            bundle.push_str(&format!("{}\n", entry.path().display()));
        }
    }

    Ok(bundle)
}
```

Wire this to a menu item or keyboard shortcut. When the user clicks "Export Diagnostics," they get a single text file they can attach to a bug report. One file, one step. The alternative is three emails back and forth asking for different pieces of information.

## Remote log collection (with consent)

For apps with telemetry, you can ship crash reports and error-level logs back to your server. This requires user consent. Do not skip that part.

The implementation is straightforward: on the next launch after a crash, check for crash report files and prompt the user.

```rust
fn check_for_unsent_crash_reports(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "log")
                && e.file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with("crash_"))
        })
        .map(|e| e.path())
        .collect()
}
```

Collect the unsent reports, show the user what will be sent (redact any paths containing usernames), and POST them to an endpoint. This closed-loop system means you hear about crashes without waiting for users to manually report them.

For the privacy side of this, I covered [telemetry with consent gating](/blog/rust-desktop-telemetry-privacy/) in a separate post.

## Conditional compilation for debug tools

Some debugging code has no business shipping in release builds. Frame-time histograms, verbose tracing, in-app log viewers: these are development tools. Use Cargo feature flags or `cfg` attributes to gate them.

```rust
#[cfg(debug_assertions)]
fn show_debug_panel(cx: &mut App) {
    // Only compiled in debug builds
}

#[cfg(feature = "diagnostics")]
fn export_diagnostics(cx: &mut App) {
    // Only compiled when the diagnostics feature is enabled
}
```

This keeps your release binary smaller and avoids shipping debug UI that confuses non-technical users. You can also enable `diagnostics` in release builds behind a menu item, giving power users access without bloating the default experience.

## What actually works

I have tried most of the debugging approaches available for Rust desktop apps. Here is what I reach for every time, in order:

1. **Structured logging to rolling files.** This solves 70% of field bugs. If you do one thing from this post, do this.
2. **Custom panic hooks with disk persistence.** You need crash reports. stderr is not a crash report.
3. **Frame-time instrumentation.** UI performance bugs are invisible without measurement.
4. **Diagnostics bundles.** Make it easy for users to give you useful information.

The tools that do not help as much as people think: debug builds sent to users (too slow, behaves differently), print-statement debugging in production (logs rotate away before you see them), and trying to reproduce every bug locally (some bugs are hardware or OS specific and you will never reproduce them without field data).

## Debugging in gpui-starter

If you want to see these techniques wired up in a working application, [gpui-starter](https://github.com/hmziqrs/gpui-boilerplate) includes structured logging, panic hooks with crash report files, frame-time profiling, and a diagnostics export system out of the box. The [getting started guide](/docs/getting-started/) walks through the setup so you can adapt the patterns to your own project.
