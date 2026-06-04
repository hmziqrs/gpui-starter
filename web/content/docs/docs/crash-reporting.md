---
title: Crash Reporting
description: Structured crash reports with stack traces, panic hooks, and next-launch surfacing.
---

When a Rust application panics, the default behavior is to print a backtrace to stderr and exit. That information disappears once the terminal closes. The crash reporting module in gpui-starter captures panics at the source, writes a structured JSON report to disk, and surfaces pending reports on the next launch so you can upload them to your own server.

This page covers the panic hook, the crash report data model, file-based and SQLite persistence, the upload pipeline, the render-path error boundary, and configuration. For the broader app lifecycle that triggers these hooks, see the [Architecture](/docs/architecture/) page. For a tutorial-style walkthrough of why crash reporting is hard in Rust desktop apps, see the [crash reporting in Rust desktop apps](/blog/rust-desktop-crash-reporting/) blog post.

## How it works

The system has four moving parts:

1. **Custom panic hook** - installed at startup, captures the panic message and backtrace, writes a JSON file to disk before the process exits.
2. **Crash marker file** - a sentinel written at startup and removed on clean shutdown. If the sentinel is still present on the next launch, the previous run crashed.
3. **Error boundary** - a thread-local guard that detects panics originating inside the render path and swaps in a fallback view instead of retrying the crashing page.
4. **Upload pipeline** - on next launch, pending reports are loaded from SQLite and POSTed to a configurable endpoint.

## Module structure

```
src/services/crash_report.rs      # CrashReport model, file I/O, upload logic
src/services/storage.rs           # SQLite persistence (crash_reports table)
src/app/lifecycle.rs              # Panic hook, crash marker, render-path tracking
src/features/pages/render_error.rs  # Error boundary fallback view
src/features/pages/diagnostics.rs   # Diagnostics page (crash report status)
src/features/pages/error_playground.rs  # Interactive error boundary test page
```

## CrashReport data model

Every panic produces a `CrashReport` with these fields:

```rust
pub struct CrashReport {
    pub id: String,              // UUID v4
    pub panic_message: String,   // The panic info string
    pub backtrace: String,       // std::backtrace::Backtrace capture
    pub app_version: String,     // CARGO_PKG_VERSION at compile time
    pub os: String,              // std::env::consts::OS
    pub arch: String,            // std::env::consts::ARCH
    pub timestamp: String,       // RFC 3339 UTC
    pub render_path: bool,       // Whether the panic originated during rendering
    pub recent_errors: Vec<String>, // Last 20 tracked error messages
}
```

The `recent_errors` field is populated by `lifecycle::track_recent_error`, which the error surface module calls each time an error is reported. This gives you a timeline of what went wrong in the seconds before the crash.

## Panic hook

The panic hook is installed early in `app::init` before any other services start:

```rust
// In src/app/mod.rs
pub fn init(cx: &mut App) {
    crate::lifecycle::install_panic_hook();
    crate::lifecycle::write_crash_marker();
    // ...
}
```

The hook wraps Rust's default panic handler. It captures the panic message, takes a `std::backtrace::Backtrace`, checks whether the panic happened inside the render path (via a thread-local flag), and writes the report to `{data_dir}/crash_reports/{id}.json` using synchronous `std::fs` calls (async I/O is not available inside a panic handler).

```rust
// Simplified from src/app/lifecycle.rs
std::panic::set_hook(Box::new(move |info| {
    let summary = info.to_string();
    let is_render = in_render_path();

    if is_render {
        RENDER_PANIC_OCCURRED.store(true, Ordering::SeqCst);
    }

    if let Some(data_dir) = APP_DATA_DIR.get() {
        let backtrace = std::backtrace::Backtrace::capture();
        let recent_errors = RECENT_ERRORS.get()
            .and_then(|slot| slot.lock().ok())
            .map(|guard| guard.clone())
            .unwrap_or_default();

        let report = CrashReport::new(
            summary.clone(),
            backtrace.to_string(),
            is_render,
            recent_errors,
        );

        let _ = write_crash_report(&report, data_dir);
    }

    previous(info); // call the original hook
}));
```

The data directory is set once via `lifecycle::set_app_data_dir` right after `app_state` is initialized, so the panic hook can write files without needing GPUI context.

## Crash marker

The crash marker is a file written to the system temp directory at startup (`gpui-starter.crash-marker`). It contains the PID and a timestamp:

```
pid=12345
started_at=2026-06-05T14:30:00Z
```

On clean shutdown, the marker is removed. If the process crashes (panic, signal, kill -9), the marker stays on disk. The next launch checks for it:

```rust
let previous_crash = crate::lifecycle::check_previous_crash();
if let Some(marker) = &previous_crash {
    tracing::warn!("previous crash detected: {}", marker);
}
```

When a previous crash is detected, the upload pipeline runs immediately to flush any pending reports.

## File-based persistence

Crash reports are written as JSON files to `{data_dir}/crash_reports/`. This happens inside the panic hook, where only synchronous I/O is safe:

```rust
pub fn write_crash_report(report: &CrashReport, data_dir: &Path) -> std::io::Result<()> {
    let reports_dir = data_dir.join("crash_reports");
    std::fs::create_dir_all(&reports_dir)?;
    let file_path = reports_dir.join(format!("{}.json", report.id));
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(&file_path, json)?;
    Ok(())
}
```

On next launch, `detect_pending_reports` scans that directory, parses each JSON file, and returns them sorted newest-first. Malformed files are skipped with a warning log.

## SQLite persistence

Reports are also persisted to a `crash_reports` table in the app's SQLite database. The table schema:

```sql
CREATE TABLE IF NOT EXISTS crash_reports (
    id TEXT PRIMARY KEY,
    panic_message TEXT NOT NULL,
    backtrace TEXT NOT NULL,
    app_version TEXT NOT NULL,
    os TEXT NOT NULL,
    arch TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    render_path BOOLEAN NOT NULL,
    recent_errors TEXT NOT NULL,  -- JSON array
    uploaded BOOLEAN NOT NULL DEFAULT 0,
    uploaded_at TEXT
);

CREATE INDEX idx_crash_reports_timestamp ON crash_reports (timestamp DESC);
CREATE INDEX idx_crash_reports_uploaded ON crash_reports (uploaded);
```

The `uploaded` column tracks which reports have been sent to the server. The upload pipeline queries `WHERE uploaded = 0` and marks each report after a successful POST.

## Upload pipeline

Pending reports are uploaded to an HTTP endpoint configured via the `GPUI_CRASH_REPORT_URL` environment variable at compile time:

```rust
let endpoint = option_env!("GPUI_CRASH_REPORT_URL").unwrap_or("").to_string();
```

The upload flow:

1. On next launch, if a previous crash was detected, `upload_pending_reports` is called.
2. It loads pending reports from SQLite (up to 50 at a time).
3. Each report is serialized as JSON and POSTed to the endpoint with `Content-Type: application/json`.
4. On success, the report is marked as `uploaded` in SQLite with a timestamp.
5. The `CrashReportSnapshot` global is updated with the current pending count and last crash timestamp.

You can also trigger a manual retry from the diagnostics page or by calling:

```rust
crash_report::upload_pending_reports(cx);
```

The `CrashReportSnapshot` global tracks the upload state:

```rust
pub struct CrashReportSnapshot {
    pub pending_count: usize,
    pub last_crash_timestamp: Option<String>,
    pub upload_endpoint: String,
    pub last_upload_error: Option<String>,
}
```

Read it from any GPUI context:

```rust
let snap = crash_report::snapshot(cx);
println!("pending: {}, last crash: {:?}",
    snap.pending_count,
    snap.last_crash_timestamp
);
```

## Error boundary

GPUI render panics are process-fatal. They propagate through an `extern "C"` Metal rendering callback where Rust's unwinder cannot safely unwind. The error boundary works around this by detecting render panics *after* they happen, on the next frame.

A thread-local flag tracks whether the current thread is inside the render path:

```rust
// In the root component's render method
let _guard = crate::lifecycle::enter_render_path();
// Only the active_page_view call is inside the guard
```

When a panic fires during rendering, the panic hook sets `RENDER_PANIC_OCCURRED` to `true`. On the next render pass, the root component checks this flag:

```rust
fn active_page_view(&mut self, cx: &mut Context<Self>) -> AnyView {
    if crate::lifecycle::take_render_panic() {
        self.render_error = true;
        let summary = crate::lifecycle::last_panic_summary()
            .unwrap_or_else(|| "An unknown error occurred.".to_string());
        self.error_page = Some(cx.new(|_| RenderErrorPage::new(summary)));
    }

    if self.render_error {
        return self.error_page.clone().unwrap().into();
    }

    self.unchecked_active_page_view()
}
```

The `RenderErrorPage` shows the panic summary and a "Reload Page" button. Navigating to a different route or clicking reload clears the error boundary and retries the original page. This is not a try/catch mechanism. The panic still kills the process eventually. The error boundary only activates when the panic happens to be caught by the unwinder before the Metal callback boundary.

For testing, the error playground page uses `TriggerRenderError` actions to simulate the boundary without a real panic. See the [Testing](/docs/testing/) page for how to write tests that exercise error states.

## Configuration

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `GPUI_CRASH_REPORT_URL` | compile-time env | `""` | HTTP endpoint for crash report uploads |
| `app_data_dir` | runtime path | platform data dir | Where crash report JSON files are stored |
| `crash_reports/` subdirectory | auto-created | inside data dir | File-based crash report storage |
| `recent_errors` ring buffer | in-memory | 20 entries | Error messages attached to crash reports |

Set the upload endpoint at build time:

```bash
GPUI_CRASH_REPORT_URL=https://api.example.com/crash-reports cargo build --release
```

When the endpoint is empty (the default), the upload pipeline is a no-op. Reports accumulate in SQLite and on disk until you configure an endpoint.

## Initialization and shutdown

The crash report service is initialized after the storage layer is ready:

```rust
// In app::init, after storage initialization
crate::lifecycle::set_app_data_dir(app_state::paths(cx).data_dir.clone());
// ... later ...
crate::crash_report::initialize(cx);
if previous_crash.is_some() {
    crate::crash_report::upload_pending_reports(cx);
}
```

On clean shutdown, the service flushes any remaining uploads:

```rust
// In the Quit action handler
crate::crash_report::shutdown(cx);  // calls upload_pending_reports one last time
crate::lifecycle::remove_crash_marker();  // removes the sentinel file
```

## Diagnostics page

The [diagnostics page](/docs/architecture/) exposes crash report status in the app. It observes `CrashReportSnapshot` and displays:

- Pending report count
- Last crash timestamp
- Configured upload endpoint
- Last upload error

It also provides a "Retry Crash Upload" button that calls `upload_pending_reports` manually. In debug builds, a "Trigger Test Panic" button is available to exercise the full crash report flow end-to-end.

## Related

- [Architecture](/docs/architecture/) - overall app structure, lifecycle stages, and service initialization order
- [Testing](/docs/testing/) - how to mock globals and test crash report logic without real panics
- [Notifications](/docs/notifications/) - another service with file-based persistence and async upload
- [Crash reporting in Rust desktop apps](/blog/rust-desktop-crash-reporting/) - tutorial on designing crash reporting for Rust GUI applications
- [Rust desktop debugging techniques](/blog/rust-desktop-debugging-techniques/) - debugging production crashes from structured logs
- [Rust desktop error boundaries](/blog/rust-desktop-error-boundaries/) - deeper look at why render panics are fatal and how to handle them
