---
question: "Does gpui-starter collect telemetry or tracking data?"
description: "No. Telemetry is disabled by default with opt-in local-only logging and optional OTLP export behind a consent gate."
category: "Advanced"
order: 18
---

No. Telemetry ships disabled and stays that way until you change it. Nothing leaves the app without an explicit opt-in.

## The short version

The telemetry module (`src/services/telemetry.rs`) has a three-mode switch and a hard consent gate. The default is `Disabled`, which means every call to `record_event` or `record_error` returns immediately without doing anything. There is no background thread, no deferred upload, no hidden buffer. The sink is a no-op.

## The three modes

- **Disabled** (default): all events are discarded at the call site. No data is recorded, buffered, or sent. This is the state on every fresh install.
- **LocalOnly**: events are written to the local `tracing` subscriber at `debug`/`warn` level under the `gpui_starter::telemetry` target. They appear in whatever log output you have configured (stderr, file, etc.). They never leave the machine. There is no network component.
- **Remote**: events are forwarded to an [OpenTelemetry Collector](https://opentelemetry.io/docs/collector/) endpoint via OTLP HTTP/Protobuf. This is the only mode that sends data externally, and it requires two things: `consented = true` and a reachable collector URL.

## The consent gate

Every mode except `Disabled` requires `consented = true` in the call to `set_mode`. If consent is `false`, the sink is replaced with `DisabledSink` regardless of which mode you asked for. The match arm that handles this is `(_, false) => DisabledSink`, so there is no code path where data is recorded without consent.

The consent state is also reflected in the capability system. When consent has not been given, the capability reason reads "telemetry disabled until consent" and shows up as `enabled: false` on the [diagnostics page](/faq/diagnostics/).

## Remote mode configuration

Remote mode uses the standard OpenTelemetry OTLP HTTP transport on port 4318. The collector endpoint is resolved in this order:

1. The explicit `endpoint` argument passed to `set_mode`.
2. The `OTEL_EXPORTER_OTLP_ENDPOINT` environment variable.
3. The built-in default: `http://localhost:4318`.

Real export requires the `otlp` Cargo feature (see the comments in `telemetry.rs` for the exact dependency versions). Without that feature flag, the remote sink logs events through `tracing` but does not ship them anywhere. This is intentional: you can compile without the feature in release builds to guarantee zero network telemetry, even if someone calls `set_mode(Remote, true, ...)`.

Connection failures during tracer installation set a `connected: false` flag on `RemoteSink`. Subsequent calls to `record_event` return `TelemetryError::NotAvailable` and the error surfaces in `TelemetrySnapshot::last_export_error`, which the diagnostics page reads.

## How to remove telemetry entirely

All telemetry logic lives behind the `TelemetrySink` trait. The rest of the codebase calls `record_event`, `record_error`, and `set_user_property` through that interface and never touches the underlying sink. To strip telemetry out completely:

1. Delete `src/services/telemetry.rs` and `src/services/telemetry.test.rs`.
2. Remove the `telemetry::initialize` call from your app setup (likely `app.rs` or `main.rs`).
3. Remove any UI that calls `set_mode`.

That is it. No other module depends on the telemetry sink directly.

## What gets recorded

The public API exposes four operations: `record_event(name)`, `record_error(error)`, `set_user_property(key, value)`, and `flush()`. In local mode, these emit `tracing::debug!` and `tracing::warn!` spans. In remote mode, they queue spans for the OTLP batch exporter. The `flush` call pushes buffered spans to the collector without shutting down the provider, so you can call it from a settings button or on a timer.

The `shutdown` function (called during app teardown) flushes remaining spans and then calls `global::shutdown_tracer_provider()`, which is a one-way operation. No further spans can be exported after shutdown.

## Related

- [Diagnostics page](/faq/diagnostics/) shows live telemetry state (mode, consent, endpoint, event count, errors).
- [Architecture](/docs/architecture/) covers how the capability system and global state work together.
- [Crash reporting for Rust desktop apps](/blog/rust-desktop-crash-reporting/) covers the separate crash reporting pipeline, which is independent of telemetry.
- [Integrating tracing in Rust](/blog/integrating-tracing-rust-logging/) covers the `tracing` subscriber setup that local telemetry mode relies on.
