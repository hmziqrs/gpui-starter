---
title: "Structured logging in Rust desktop apps with tracing"
description: "Setting up structured logging in Rust desktop apps with the tracing crate: file output, filtering, and async-aware logging."
date: 2026-06-05
tags: [Rust, logging, tracing]
draft: false
---

Most Rust programs start with `println!` and graduate to the `log` crate. That works fine for CLI tools and small services. Desktop applications are a different problem. A GUI app runs for hours or days. Bugs show up in the field, not on your machine. When something goes wrong at 2 AM on a user's laptop, you need structured, queryable logs written to disk, not ephemeral terminal output.

The `tracing` crate is the standard answer in the Rust ecosystem. It extends `log` with spans, structured fields, and first-class async support. This post covers how to wire it up in a real desktop application: dual output to stdout and rolling files, per-module filtering, and the non-obvious detail about keeping the writer guard alive.

## Why tracing over log

The `log` crate gives you five macros (`error!`, `warn!`, `info!`, `debug!`, `trace!`) and a trait for backends. That is enough for simple cases. `tracing` adds two things that matter for long-running apps.

First, structured fields. Instead of formatting a string, you attach key-value pairs to events:

```rust
tracing::warn!(
    target: "gpui_starter::lifecycle",
    path = %path.display(),
    error = %err,
    "failed to write crash marker"
);
```

The target, path, and error are separate fields. A log aggregator can filter on `error` without parsing the message string. You can grep for them in a file too, but structured data scales better when you have thousands of log lines from a single session.

Second, spans. A span represents a unit of work and carries context across multiple events:

```rust
use tracing::instrument;

#[instrument(skip(cx), fields(step = %step_name))]
fn measure_step(step_name: &str, cx: &mut App) {
    let start = std::time::Instant::now();
    // ... do work ...
    tracing::info!(
        target: "gpui_starter::startup",
        elapsed_ms = start.elapsed().as_millis() as u64,
        "step complete"
    );
}
```

Every event inside this span inherits the `step` field. In a desktop app where startup involves a dozen initialization steps, this context is the difference between "something took 200ms" and "theme_init took 200ms."

## Setting up dual output: stdout and rolling files

A desktop app needs two things from its logging. During development, stdout is enough. In production, logs have to go to a file on disk so users can send them to you when they file a bug report.

`tracing-subscriber` handles this with layers. Each layer is an independent output destination with its own formatting and filtering.

```rust
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

pub fn initialize_logging(log_dir: &Path) -> Result<WorkerGuard, Box<dyn std::error::Error>> {
    // Rolling daily log files: app.log.2026-06-05, app.log.2026-06-06, etc.
    let file_appender = tracing_appender::rolling::daily(log_dir, "app.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        // Layer 1: stdout with ANSI colors (development)
        .with(tracing_subscriber::fmt::layer())
        // Layer 2: file output, no ANSI escape codes
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(file_writer),
        )
        .with(build_env_filter())
        .try_init()?;

    Ok(guard)
}
```

The `WorkerGuard` return value is critical. `tracing_appender::non_blocking` spawns a background thread that drains the write buffer. When the guard is dropped, the thread shuts down and flushes remaining data. If you drop the guard too early, you lose logs. I store it in a global state struct that lives for the entire application lifetime.

```rust
pub struct LoggingState {
    pub runtime: LoggingRuntime,
    guard: Option<WorkerGuard>,  // held until shutdown
}
```

This is the kind of detail that does not show up in the crate documentation prominently. I learned about it the hard way, after wondering why log files were empty on shutdown.

## Filtering: controlling what gets logged

Running a GUI app at `trace` level produces an absurd amount of output. GPUI alone emits hundreds of events per frame. The solution is `EnvFilter`, which reads the `RUST_LOG` environment variable and lets you override it with programmatic directives.

```rust
use tracing_subscriber::EnvFilter;

fn build_env_filter() -> EnvFilter {
    EnvFilter::from_default_env()
        // Silence GPUI's noisy window internals
        .add_directive("gpui::window=off".parse().unwrap())
        // Our own code at full trace level
        .add_directive("gpui_starter=trace".parse().unwrap())
        // Notification library at debug
        .add_directive("notify_rust=debug".parse().unwrap())
}
```

This gives you multiple levels of control. The `RUST_LOG` environment variable works at runtime for ad-hoc debugging. The hardcoded directives silence known-noisy crates at compile time. And you can add or remove directives when you initialize the subscriber based on build configuration.

During development, run the app with `RUST_LOG=debug cargo run` and you get everything. In release builds, the hardcoded directives keep the log files reasonable. A typical session generates maybe 2-3 MB per day with daily rotation.

## Logging from panic hooks

Desktop apps crash. Rust catches panics, but the default panic handler writes to stderr and exits. If you want crash data in your log files, you need to install a custom panic hook that logs before calling the original handler.

```rust
use std::panic;

pub fn install_panic_hook(data_dir: PathBuf) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let summary = info.to_string();

        tracing::error!(
            target: "gpui_starter::lifecycle",
            panic = %summary,
            "application panic captured"
        );

        // Write a crash report file for the next launch to find
        if let Ok(report) = generate_crash_report(info) {
            let _ = fs::write(
                data_dir.join("crash-report.txt"),
                &report,
            );
        }

        previous(info);
    }));
}
```

One detail worth noting: call `previous(info)` at the end. You want the original panic behavior (abort or unwind) to still happen. Your hook just gets a chance to log first. The `%summary` formatting operator calls `Display` on the panic info, which gives you the panic message and location.

We do something similar in [gpui-starter's crash reporting system](/blog/building-desktop-apps-rust-gpui/), where a crash marker file is written at startup and removed on clean shutdown. If the marker exists on the next launch, the app knows the previous run crashed and can offer to send the report.

## A note on async and spans

GPUI uses its own async runtime, but the principle is the same with tokio or any other runtime. `tracing` spans follow the logical task, not the OS thread. When you instrument an async function:

```rust
#[tracing::instrument(skip(db))]
async fn fetch_user(db: &Database, user_id: u64) -> Result<User, Error> {
    tracing::debug!(user_id, "querying database");
    let user = db.query(user_id).await?;
    tracing::info!(user_id, name = %user.name, "user fetched");
    Ok(user)
}
```

The span is entered when the future is polled and exited when it yields. This means the `user_id` field appears on every log line inside the function, even if the runtime moves the task between threads between `await` points. This is the main reason to prefer `tracing` over `log` in any async codebase. The context follows the logical flow of your program.

For desktop apps specifically, this matters when you have background tasks like auto-updates or file watchers running concurrently with the UI thread. You can trace a single update download across multiple await points without losing track of which version was being downloaded.

## Shutdown: flushing the buffer

When a desktop app quits, you need to flush any buffered log writes. The `WorkerGuard` handles this when dropped, but you want to be explicit about the order. Flush your logs before you tear down the rest of the application.

```rust
pub fn shutdown(cx: &mut App) {
    if let Some(state) = cx.try_global::<LoggingState>() {
        tracing::debug!(
            target: "gpui_starter::logging",
            enabled = state.runtime.enabled,
            "logging shutdown requested"
        );
        cx.update_global::<LoggingState, _>(|state, _cx| {
            state.guard = None;  // drops the guard, flushes the writer
        });
    }
}
```

Dropping the guard to `None` triggers the flush. This is one of those places where Rust's RAII semantics work in your favor. You do not need a separate `flush()` call. The `Drop` implementation on `WorkerGuard` handles it.

## What about the log crate compatibility?

`tracing` has a compatibility layer through the `tracing-log` crate. It forwards any `log::info!()` calls from dependencies to your tracing subscriber. This means libraries that use `log` (and most Rust libraries do) work transparently. You do not need to convert your entire dependency tree.

Add `tracing-log` to your `Cargo.toml` and call `tracing_log::LogTracer::init()` before setting up your subscriber. After that, every `log::warn!()` call from any crate appears as a tracing event with the correct level and target.

## Common mistakes I have seen

Dropping the guard too early is the most common one. I already mentioned this. The `non_blocking` writer returns a guard. Store it somewhere that outlives the rest of your application. A static or a global is fine.

The `target:` parameter in tracing macros is free metadata. Use it to namespace your log events by module or subsystem. `gpui_starter::startup` and `gpui_starter::lifecycle` are more useful than `gpui_starter` when you are grepping through a 50 MB log file.

GPUI renders at 60+ frames per second. A `tracing::info!()` inside a render callback will fill your log file in minutes. Keep hot paths at `trace!` or `debug!` and use the filter to keep them off by default.

In tracing macros, `%` calls `Display` and `?` calls `Debug`. If you write `error = err` without a formatter, you get a compile error. This is actually good: it forces you to be explicit about how each field is rendered.

---

The setup described here matches what [gpui-starter](/docs/getting-started/) ships with out of the box: dual output to stdout and rolling daily files, per-crate filtering, crash-aware panic hooks, and clean shutdown flushing. You can see the full implementation in the [logging service on GitHub](https://github.com/freeoxide/gpui-starter) and the [desktop app patterns in the docs](/docs/getting-started/).
