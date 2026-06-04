---
title: Telemetry
description: Privacy-first telemetry with three modes (disabled, local, remote) and explicit consent.
---

The telemetry module (`src/services/telemetry.rs`) collects application events and errors without shipping data behind the user's back. It starts disabled and stays that way until the user gives explicit consent. Three modes are available: fully off, local logging, and remote OTLP export. The capability system surfaces connection problems in the diagnostics page.

This page covers the mode system, the sink trait, endpoint resolution, Cargo feature flags, and the public API. For the capability system that tracks telemetry status app-wide, see [Architecture](/docs/architecture/). For the diagnostics page that displays telemetry state at runtime, see [Testing](/docs/testing/).

## How it works

Telemetry is built around a `TelemetrySink` trait and three implementations:

| Sink | Mode | What happens |
|------|------|-------------|
| `DisabledSink` | `Disabled` | All calls return `Ok(())`. Nothing is logged or sent. |
| `LocalSink` | `LocalOnly` | Events and errors are emitted through `tracing` at debug/warn level. No network calls. |
| `RemoteSink` | `Remote` | Events flow through `tracing` and the OpenTelemetry pipeline to an OTLP collector over HTTP/Protobuf. |

When consent is `false`, the mode is ignored and `DisabledSink` is used regardless. This is enforced in `set_mode`:

```rust
let enabled = consented && mode != TelemetryMode::Disabled;
```

The sink is swapped at runtime by calling `set_mode`. No restart required.

## Telemetry modes

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelemetryMode {
    Disabled,
    LocalOnly,
    Remote,
}
```

**Disabled**: the default. No data collected. Every `record_event` and `record_error` call is a no-op. Use this when the user has not given consent, or when they explicitly opt out.

**LocalOnly**: events and errors are written to the `tracing` subscriber. This is useful during development or when you want structured logs without network delivery. The subscriber configuration (set elsewhere in the app) controls where the logs actually go: stdout, a file, or both.

**Remote**: events are forwarded to an OTLP collector over HTTP/Protobuf on port 4318. The collector can be local (the default `http://localhost:4318`) or remote. The `otlp` Cargo feature must be enabled for real export. Without it, `RemoteSink` logs the endpoint and records events through tracing, but does not ship spans.

## Initialization

Call `telemetry::initialize(cx)` during app startup, before any code that might record events:

```rust
use crate::services::telemetry;

// In your app init function
telemetry::initialize(cx);
```

This installs two GPUI globals: `TelemetrySnapshot` (read-only state for UI rendering) and `TelemetryRuntime` (holds the current sink behind an `Arc`). The snapshot starts with `consented: false`, `enabled: false`, and `mode: Disabled`.

## Setting the mode

```rust
use crate::telemetry::{self, TelemetryMode};

// User consented and picked local-only logging
telemetry::set_mode(TelemetryMode::LocalOnly, true, None, cx);

// User wants remote export to a custom collector
telemetry::set_mode(
    TelemetryMode::Remote,
    true,
    Some("https://telemetry.example.com:4318"),
    cx,
);

// User opted out
telemetry::set_mode(TelemetryMode::Disabled, false, None, cx);
```

The `endpoint` parameter is only used when `mode` is `Remote`. When `None`, the endpoint is resolved from the environment or the built-in default. See endpoint resolution below.

## Recording events and errors

```rust
// Record a named event (e.g. "user_opened_settings", "export_completed")
telemetry::record_event("settings_test_event", cx);

// Record an error that should appear in telemetry
telemetry::record_error("settings_test_error", cx);

// Attach a user property for segmentation
telemetry::set_user_property("plan_phase", "phase21", cx);
```

Each call goes through the current sink. On success, `TelemetrySnapshot::events_recorded` increments. On failure (e.g. the OTLP exporter is disconnected), the error is captured in `last_export_error` and `last_error` on the snapshot.

## Flushing and shutdown

```rust
// Push pending spans to the collector without disabling telemetry.
// Safe to call repeatedly (e.g. from a "Flush Telemetry" button).
telemetry::flush(cx);

// Flush and then permanently shut down the tracer provider.
// No further spans can be exported after this. Call once at app exit.
telemetry::shutdown(cx);
```

`flush` calls `force_flush` on the SDK tracer provider. The provider stays active and continues accepting new spans. `shutdown` calls `flush` first, then terminates the global tracer provider via `global::shutdown_tracer_provider()`. This is a one-way operation. Call it once during app shutdown.

## Reading telemetry state

The snapshot is a GPUI global. Any component can read it:

```rust
let snap = telemetry::snapshot(cx);

println!("enabled: {}", snap.enabled);
println!("consented: {}", snap.consented);
println!("mode: {:?}", snap.mode);
println!("endpoint: {:?}", snap.endpoint_redacted);
println!("events recorded: {}", snap.events_recorded);
println!("last error: {:?}", snap.last_error);
```

### TelemetrySnapshot fields

| Field | Type | Description |
|-------|------|-------------|
| `compiled` | `bool` | Whether telemetry code compiled in (always `true` in standard builds). |
| `consented` | `bool` | Whether the user gave explicit consent. |
| `enabled` | `bool` | `consented && mode != Disabled`. The effective on/off state. |
| `mode` | `TelemetryMode` | Current operating mode. |
| `endpoint_redacted` | `Option<String>` | Collector host with path stripped (e.g. `telemetry.example.com/...`). `None` in disabled/local modes. |
| `events_recorded` | `u64` | Running count of successful record calls since last mode change. |
| `last_export_error` | `Option<String>` | Most recent error from the OTLP exporter (connection failures, flush errors). |
| `last_error` | `Option<String>` | Most recent error from any sink operation. |

## Endpoint resolution

When `set_mode` is called with `mode: Remote`, the collector endpoint is resolved in this order:

1. The `endpoint` argument passed to `set_mode`.
2. The `OTEL_EXPORTER_OTLP_ENDPOINT` environment variable.
3. The built-in default: `http://localhost:4318`.

```rust
// Explicit endpoint
telemetry::set_mode(TelemetryMode::Remote, true, Some("https://my-collector:4318"), cx);

// Let the env var or default decide
telemetry::set_mode(TelemetryMode::Remote, true, None, cx);
```

The resolved endpoint is stored in redacted form on the snapshot for display in diagnostics. Only the hostname is kept; the path is replaced with `...`.

## Cargo feature: `otlp`

Real OTLP export requires the `otlp` feature:

```toml
[features]
otlp = ["dep:opentelemetry-otlp", "dep:opentelemetry_sdk"]
```

When the feature is disabled (the default), `install_otlp_tracer` is a no-op that logs the endpoint at debug level and returns success. `RemoteSink` sets `connected: true` because the tracing-only path always works. This means you can set `mode: Remote` without the feature and events will still flow through `tracing`; they just will not reach a collector.

When the feature is enabled, `install_otlp_tracer` creates a real `TracerProvider` with an HTTP/Protobuf exporter and installs it on the global OpenTelemetry pipeline. The provider is held by `RemoteSink` so `force_flush` can push pending spans without shutting down. The feature pulls in `opentelemetry-otlp` (with `http-proto` and `reqwest-client` features) and `opentelemetry_sdk` (with `rt-tokio` and `trace`). See `Cargo.toml` for pinned versions.

Build with the feature:

```bash
cargo build --features otlp
```

## Capability system integration

Every mode change calls `set_capability`, which registers the `telemetry` capability with the app-wide capability registry:

```rust
crate::capabilities::set(
    "telemetry",
    crate::capabilities::CapabilityStatus {
        supported: snapshot.compiled,
        enabled: snapshot.enabled,
        degraded: snapshot.last_error.is_some(),
        reason: if !snapshot.consented {
            Some("telemetry disabled until consent".into())
        } else {
            None
        },
        last_error: snapshot.last_error.clone().map(Into::into),
    },
    cx,
);
```

The diagnostics page reads this capability to display telemetry status alongside every other subsystem. See [Architecture](/docs/architecture/) for how the capability system works.

## Custom sinks

The `TelemetrySink` trait has four methods:

```rust
pub trait TelemetrySink: Send + Sync {
    fn record_event(&self, name: &str) -> Result<(), TelemetryError>;
    fn record_error(&self, error: &str) -> Result<(), TelemetryError>;
    fn set_user_properties(&self, key: &str, value: &str) -> Result<(), TelemetryError>;
    fn flush(&self) -> Result<(), TelemetryError>;
}
```

To add a custom sink (for example, one that writes to SQLite or posts to an internal analytics endpoint), implement the trait and wire it into `set_mode` by extending the match arm. The existing tests show the pattern: `FakeSink` in `telemetry.test.rs` implements `TelemetrySink` with an in-memory `Vec` for assertion.

## Testing

The test file (`src/services/telemetry.test.rs`) uses a `FakeSink` that stores calls in a `Mutex<Vec<String>>`. This lets you verify recording behavior without a real collector:

```rust
let events = Arc::new(Mutex::new(Vec::new()));
let sink = FakeSink { events: events.clone() };
sink.record_event("evt").expect("event");
sink.record_error("oops").expect("error");

let data = events.lock().expect("lock").clone();
assert_eq!(data, vec!["evt", "err:oops"]);
```

Additional tests cover endpoint resolution precedence, redaction logic, and the `RemoteSink` behavior with and without the `otlp` feature. See [Testing](/docs/testing/) for the full GPUI test harness setup.

## Common patterns

### Consent gate in settings UI

```rust
// Only show the "Remote" button after consent is given
let snap = telemetry::snapshot(cx);
if snap.consented {
    // Show mode selector (LocalOnly / Remote)
} else {
    // Show consent prompt first
}
```

### Flush before shutdown

```rust
// In your app's shutdown path
telemetry::flush(cx);
// Other cleanup...
telemetry::shutdown(cx);
```

### Check for degraded state

```rust
let snap = telemetry::snapshot(cx);
if snap.last_export_error.is_some() {
    // Show a warning: the collector is unreachable
}
```

## Related docs

- [Architecture](/docs/architecture/): capability system, entity patterns, async boundaries
- [Testing](/docs/testing/): GPUI test harness, mocking globals, asserting on snapshot state
- [Performance](/docs/performance/): tracing subscriber configuration, log filtering
