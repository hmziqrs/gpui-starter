---
title: Diagnostics Page
description: Live view of app state, subsystem health, theme metadata, and debug actions.
---

The diagnostics page (`src/features/pages/diagnostics.rs`) is a read-only dashboard that reads from 14 GPUI globals and displays key/value rows covering lifecycle stage, notification backend, storage health, connectivity, telemetry, crash reports, undo stack depth, registered commands, capability status, and file system paths. Six action buttons let you refresh the view, copy a text summary to the clipboard, open the logs folder, dismiss the latest error, retry crash uploads, and reset the first-run flag.

Desktop apps accumulate state across many independent subsystems. When something goes wrong during development or in a user bug report, you need one place to see the full picture. The clipboard export produces a single-line summary that users can paste into GitHub issues.

For the broader architecture, see [Architecture](/docs/architecture/). For the debugging workflow this page supports, see [Rust desktop debugging techniques](/blog/rust-desktop-debugging-techniques/).

## Module location

The page is a standard GPUI render component at `src/features/pages/diagnostics.rs`. It registers as route `gpui-starter://diagnostics` in the sidebar router. See [Routing](/docs/routing/) for how pages are mapped to URLs.

## What the page displays

The page builds a list of label/value rows by reading snapshots from GPUI globals. The rows are grouped into these categories:

| Category | Source global | Example rows |
|----------|--------------|--------------|
| App metadata | Compile-time env vars | App name, version |
| Lifecycle | `LifecycleState` | Stage (Starting/Running/ShuttingDown/Crashed), startup step, shutdown step, panic summary |
| Notifications | `NativeNotificationState` | Active backend, permission state, degraded reason |
| Connectivity | `connectivity::snapshot()` | Online/offline, probe URL, last error |
| Secure storage | `secure_storage::snapshot()` | Available, last error |
| Session | `session::snapshot()` | Session state |
| Commands | `commands::registry()` | Registered count, titles, per-command availability and disabled reason |
| Logging | `logging::snapshot()` | Enabled, guard active, log directory, file prefix, last error |
| Storage | `storage::snapshot()` | Available, healthy, DB path, schema version, last maintenance, last migration, error |
| Telemetry | `telemetry::snapshot()` | Compiled, consented, enabled, mode, endpoint, events recorded, export error |
| Accessibility | `accessibility::snapshot()` | AccessKit linked, bridge enabled, status |
| Desktop actions | `desktop_actions::snapshot()` | Clipboard/picker/opener available, active watchers, last error |
| Undo/redo | `undo_stack::snapshot()` | Past/future stack size, last label, last timestamp, last rejected |
| Shortcuts | `shortcuts::snapshot()` | Enabled, registered, accelerator, error |
| Error surface | `error_surface::snapshot()` | Error count, latest error message |
| Crash reports | `crash_report::snapshot()` | Pending count, last timestamp, upload endpoint, last upload error |
| App paths | `app_state::AppState` | Config dir, data dir, cache dir, log dir, state file, active route |
| Capabilities | `CapabilityRegistry` | Per-capability: supported, enabled, degraded, reason, error |

### Capability rows

Capabilities are registered by each subsystem during initialization via `capabilities::set()`. The diagnostics page iterates the full `CapabilityRegistry` and renders one row per capability. Each row shows five fields in a compact format:

```
Capability:native_notifications: supported=true enabled=true degraded=false reason=- error=-
```

See the [Architecture](/docs/architecture/) page for how the capability system works.

## Action buttons

The page provides these buttons:

| Button | Behavior |
|--------|----------|
| Refresh | Emits `AppEventKind::DiagnosticsChanged` via the app event bus, which triggers global observers to refresh their state |
| Reset First-Run | Calls `first_run::reset(cx)` to force the onboarding flow on next launch |
| Copy Diagnostics | Calls `desktop_actions::copy_diagnostics(cx)` to copy a one-line summary to the system clipboard |
| Open Logs Folder | Calls `desktop_actions::open_logs_folder(cx)` to reveal the log directory in Finder/Explorer |
| Dismiss Latest Error | Calls `error_surface::dismiss()` on the most recent error surface entry |
| Retry Crash Upload | Calls `crash_report::upload_pending_reports(cx)` to retry any pending crash report uploads |

The "Trigger Test Panic" button appears only in debug builds (guarded by `cfg!(debug_assertions)`). It dispatches a `TriggerTestPanic` action to verify crash report generation works end-to-end.

## How it works

### Global subscriptions

`DiagnosticsPage::new()` calls `cx.observe_global_in` for each global it reads. When any global mutates, the page re-renders via `cx.notify()`. The dashboard updates in real time as subsystems change state: connectivity drops, notification permission is granted, a crash report is written.

```rust
subscriptions.push(
    cx.observe_global_in::<storage::StorageSnapshot>(window, |_, _, cx| {
        cx.notify();
    }),
);
subscriptions.push(
    cx.observe_global_in::<telemetry::TelemetrySnapshot>(window, |_, _, cx| {
        cx.notify();
    }),
);
```

The subscriptions are stored in `_subscriptions: Vec<Subscription>`. GPUI drops the observation when the `Subscription` is dropped, which happens when the page is unmounted.

### Rendering

The `Render` implementation reads every snapshot, builds a `Vec` of rows, and renders them in a vertical flex container. Each row is a bold label followed by the value string. Fixed entries come first, then path rows if `AppState` is available, then one row per registered capability.

```rust
let mut rows = vec![
    row("App", env!("CARGO_PKG_NAME")),
    row("Version", env!("CARGO_PKG_VERSION")),
    row("Lifecycle", lifecycle_label),
    // ... more rows
];

for (name, status) in capabilities {
    rows.push(row(&format!("Capability:{name}"), &format!(
        "supported={} enabled={} degraded={}",
        status.supported, status.enabled, status.degraded,
    )));
}
```

### Clipboard export

`desktop_actions::copy_diagnostics(cx)` calls `build_diagnostics_text`, which formats a single-line string with app name, version, active route, lifecycle stage, and connectivity state. This is intentionally minimal so it fits in a bug report or chat message.

```rust
fn build_diagnostics_text(cx: &App) -> String {
    let app_state = crate::app_state::config(cx);
    let stage = cx.try_global::<LifecycleState>()
        .map(|s| s.stage)
        .unwrap_or(LifecycleStage::Starting);
    let connectivity = crate::connectivity::snapshot(cx);
    format!(
        "app={} version={} route={} lifecycle={:?} connectivity={:?}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        app_state.active_route.to_url(),
        stage,
        connectivity.state,
    )
}
```

## Accessing the page

The diagnostics page is available at URL `gpui-starter://diagnostics`. It appears in the sidebar navigation and in the command launcher (Cmd+K). Search for "Diagnostics" in the command palette to navigate there directly.

Three commands are also available from the command palette without opening the page:

| Command | Effect |
|---------|--------|
| Copy Diagnostics | Clipboard export from any context |
| Open Logs Folder | Reveal logs in system file manager |
| Open Config Folder | Reveal config directory in system file manager |

## Adding custom rows

To surface a new subsystem on the diagnostics page, follow this pattern:

1. Create a snapshot struct for your subsystem (e.g., `MySnapshot`) and install it as a GPUI global.
2. Implement a `snapshot(cx: &App) -> MySnapshot` function that reads the global.
3. In `diagnostics.rs`, add a `cx.observe_global_in` subscription in `DiagnosticsPage::new`.
4. In the `Render` implementation, call your `snapshot(cx)` function and push `row()` entries into the `rows` vector.

Example:

```rust
// In DiagnosticsPage::new
subscriptions.push(
    cx.observe_global_in::<my_subsystem::MySnapshot>(window, |_, _, cx| {
        cx.notify();
    }),
);

// In Render
let my_snap = my_subsystem::snapshot(cx);
rows.push(row("My Subsystem Status", &my_snap.status));
rows.push(row("My Subsystem Error", my_snap.last_error.as_deref().unwrap_or("None")));
```

## Extending the clipboard export

`build_diagnostics_text` in `src/services/desktop_actions.rs` formats the clipboard string. To add more fields, extend the `format!` call with additional key/value pairs. Keep the output to one line. The full dump lives on the diagnostics page itself.

## Related docs

- [Architecture](/docs/architecture/): how globals, capabilities, and the event bus tie together
- [Notifications](/docs/notifications/): the notification subsystem whose state appears here
- [Secure Storage](/docs/secure-storage/): credential storage status row
- [Testing](/docs/testing/): how to mock globals when testing pages that read diagnostics data
- [Themes](/docs/themes/): theme metadata surfaced through app state

## Related blog posts

- [Rust desktop debugging techniques](/blog/rust-desktop-debugging-techniques/): using the diagnostics page and log output to diagnose issues
- [Rust desktop crash reporting](/blog/rust-desktop-crash-reporting/): how crash reports are generated and how the retry upload button works
- [Rust desktop error boundaries](/blog/rust-desktop-error-boundaries/): the error surface system that the "Dismiss Latest Error" button interacts with
- [Rust desktop lifecycle management](/blog/rust-desktop-lifecycle-management/): lifecycle stages shown on the page
- [Rust desktop telemetry and privacy](/blog/rust-desktop-telemetry-privacy/): telemetry snapshot fields and what they mean
