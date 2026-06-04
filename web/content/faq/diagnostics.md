---
question: "How do I debug or view the internal state of a GPUI app?"
description: "gpui-starter ships a diagnostics page that exposes live app state across every subsystem: lifecycle, storage, telemetry, connectivity, crash reports, and more."
category: "Advanced"
order: 17
---

Yes. Open the Diagnostics page from the sidebar (the info icon). It renders a live key/value table pulled from every subsystem in the app. No setup required, no extra dependencies to install.

## When the diagnostics page is useful

You will reach for it in a handful of situations:

- A user reports a crash and you need to know whether crash reports were captured and uploaded successfully.
- Storage migration failed silently and you want to see the schema version and last error without digging through log files.
- Telemetry is not firing and you need to check whether consent was granted, whether the compile flag is on, and what the last export error was.
- Notifications stopped working and you need the permission status and degraded reason.
- You are developing a new subsystem and want to verify that your capability reports correctly as supported/enabled.

## What each section covers

The page reads from 15 subsystems. Here is what each one tells you:

App and lifecycle: Package name, version, active route. Lifecycle stage (Starting, Running, ShuttingDown, Crashed), startup and shutdown step, and the last panic summary if the app recovered from a crash. This is the first place to look when the app fails to start or exits unexpectedly. The crash reporting system that feeds this data is covered in the [crash reporting guide](/blog/rust-desktop-crash-reporting/).

Connectivity: Network probe URL, current connection state, and the last error from a failed probe. Useful when remote features like auto-update or telemetry seem broken.

Storage: Database path, schema version, health flag, last maintenance timestamp, and last migration result. If a migration fails, the error surfaces here. The [data storage FAQ](/faq/data-storage/) explains the schema and migration system in detail.

Telemetry: Compiled-in flag, consent status, enabled status, current mode (Disabled/Local/Remote), endpoint URL (redacted), event count, and any export errors. The [telemetry FAQ](/faq/telemetry/) covers the consent gate and removal process.

Notifications: Active backend, OS permission status, and degraded reason. If the OS denied notification permissions, the degraded reason will say so.

Secure storage: OS keyring availability and any errors from the last read or write. See the [secure storage FAQ](/faq/secure-storage/) for how the keyring integration works.

Session: Current session representation, which is useful when debugging single-instance enforcement or state restore issues.

Commands: Total registered command count, each command title, and per-command availability (enabled/disabled with reason). This helps when a [keyboard shortcut](/faq/keyboard-shortcuts/) or [command launcher](/docs/command-launcher/) action is not showing up.

Accessibility: AccessKit linkage and accessibility bridge status. Covered in the [Rust desktop accessibility guide](/blog/integrating-accesskit-rust-accessibility/).

Desktop actions: Clipboard, file picker, and file opener availability, plus active filesystem watcher count and any errors.

Undo/redo: Stack sizes and the label and timestamp of the last undo entry. The [undo/redo FAQ](/faq/undo-redo-support/) explains how the stack works.

Error surface: Current error count and the latest error message. These are the errors caught by the [error boundary system](/blog/rust-desktop-error-boundaries/).

Crash reports: Pending upload count, last crash timestamp, upload endpoint, and last upload error. When crash reports pile up because the endpoint is unreachable, this section tells you why.

File paths: Config, data, cache, and log directories. State file path and any load/save errors.

Capabilities: Per-capability status showing supported, enabled, and degraded flags with reasons. Each subsystem exposes a capability descriptor that the diagnostics page aggregates.

## Actions

The page has six buttons:

- Refresh re-emits the `DiagnosticsChanged` event, causing every snapshot to be re-read from the live globals.
- Copy Diagnostics serializes the full table to the system clipboard. Paste it into a bug report or support email.
- Open Logs Folder opens the log directory in Finder (macOS) or the default file manager. The [debugging techniques post](/blog/rust-desktop-debugging-techniques/) explains the structured logging setup that writes to these files.
- Reset First-Run clears the first-run flag so the onboarding flow triggers again on next launch.
- Dismiss Latest Error removes the most recent error from the error surface.
- Retry Crash Upload re-attempts uploading any pending crash reports to the configured endpoint.

In debug builds (`cfg!(debug_assertions)`), a Trigger Test Panic button appears. It fires a deliberate panic so you can verify that the crash hook captures the report and the diagnostics page picks up the pending count on next launch.

## How it works internally

The diagnostics page subscribes to every global state type using `cx.observe_global_in`. When any subsystem updates its global, the diagnostics page re-renders. There is no polling. The `Render` implementation calls `snapshot()` on each module and builds the row list. You can see the full implementation in `src/features/pages/diagnostics.rs`.

To add a new subsystem to the diagnostics page, expose a `snapshot(cx) -> SomeSnapshot` function from your module, add an `observe_global_in` subscription in the constructor, and push new `row()` calls in `Render`. The [architecture docs](/docs/architecture/) explain how globals and the entity system connect.

## Performance note

The diagnostics page is not in the critical render path. It only exists as a route you navigate to, so the snapshot functions run on demand rather than on every frame. If you are investigating frame-time issues instead, the [performance profiling post](/blog/rust-desktop-performance-profiling/) covers the frame timer instrumentation that measures render cost.
