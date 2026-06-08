//! Telemetry service: opt-in telemetry with local-only and remote (OTLP) modes.

mod sink;

use std::sync::Arc;

use gpui::{App, BorrowAppContext as _, Global};
use opentelemetry::global;
use tracing_opentelemetry as _;

use sink::{DisabledSink, LocalSink, RemoteSink};

/// Default OTLP HTTP endpoint used when no explicit endpoint is provided.
///
/// Matches the standard OpenTelemetry Collector default for HTTP/Protobuf
/// transport on port 4318.
const DEFAULT_OTLP_ENDPOINT: &str = "http://localhost:4318";

/// Environment variable name for overriding the OTLP exporter endpoint.
///
/// Set `OTEL_EXPORTER_OTLP_ENDPOINT` to your collector URL, e.g.
/// `https://telemetry.example.com:4318`. When unset, [`DEFAULT_OTLP_ENDPOINT`]
/// is used.
const ENV_OTLP_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

/// Service name advertised to the OTLP collector in the telemetry resource.
#[cfg(feature = "otlp")]
const SERVICE_NAME: &str = "gpui-starter";

// ---------------------------------------------------------------------------
// OTLP exporter (feature-gated)
// ---------------------------------------------------------------------------
//
// To enable real OTLP export, add the following to Cargo.toml and then pass
// `--features otlp` (or set `default-features = true` below):
//
//     [features]
//     otlp = ["dep:opentelemetry-otlp", "dep:opentelemetry_sdk"]
//
//     [dependencies]
//     opentelemetry-otlp = { version = "0.17.0", optional = true, features = [
//         "http-proto",         # HTTP/Protobuf transport (no gRPC/tonic needed)
//         "reqwest-client",     # Use the existing reqwest dependency as HTTP client
//     ] }
//     opentelemetry_sdk = { version = "0.24.1", optional = true, features = [
//         "rt-tokio",           # Tokio runtime for batch exporter
//         "trace",              # Trace pipeline support
//     ] }
//
// The versions above are pinned to match the opentelemetry 0.24.x line already
// present in this crate. Upgrade in lockstep if you bump opentelemetry.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelemetryMode {
    Disabled,
    LocalOnly,
    Remote,
}

#[derive(Clone, Debug)]
pub struct TelemetrySnapshot {
    pub compiled: bool,
    pub consented: bool,
    pub enabled: bool,
    pub mode: TelemetryMode,
    pub endpoint_redacted: Option<String>,
    pub events_recorded: u64,
    pub last_export_error: Option<String>,
    pub last_error: Option<String>,
}

impl Default for TelemetrySnapshot {
    fn default() -> Self {
        Self {
            compiled: true,
            consented: false,
            enabled: false,
            mode: TelemetryMode::Disabled,
            endpoint_redacted: None,
            events_recorded: 0,
            last_export_error: None,
            last_error: None,
        }
    }
}

impl Global for TelemetrySnapshot {}

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("telemetry not available: {0}")]
    NotAvailable(String),
    #[error("OTLP error: {0}")]
    Otlp(#[source] Box<dyn std::error::Error + Send + Sync>),
}

pub trait TelemetrySink: Send + Sync {
    fn record_event(&self, name: &str) -> Result<(), TelemetryError>;
    fn record_error(&self, error: &str) -> Result<(), TelemetryError>;
    fn set_user_properties(&self, key: &str, value: &str) -> Result<(), TelemetryError>;
    fn flush(&self) -> Result<(), TelemetryError>;
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct TelemetryRuntime {
    sink: Arc<dyn TelemetrySink>,
}

impl Global for TelemetryRuntime {}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn initialize(cx: &mut App) {
    let snapshot = TelemetrySnapshot::default();
    let runtime = TelemetryRuntime {
        sink: Arc::new(DisabledSink),
    };
    set_capability(&snapshot, cx);
    cx.set_global(snapshot);
    cx.set_global(runtime);
}

pub fn snapshot(cx: &App) -> TelemetrySnapshot {
    cx.try_global::<TelemetrySnapshot>()
        .cloned()
        .unwrap_or_default()
}

/// Set the telemetry mode, consent flag, and optional endpoint override.
///
/// When `mode` is [`TelemetryMode::Remote`] and `consented` is `true`, the
/// function resolves the OTLP endpoint (see [`resolve_otlp_endpoint`]),
/// installs the tracer provider, and wires the [`RemoteSink`].
///
/// `endpoint` is optional. When `None`, the value of the
/// `OTEL_EXPORTER_OTLP_ENDPOINT` environment variable is used, falling back
/// to the built-in default.
pub fn set_mode(mode: TelemetryMode, consented: bool, endpoint: Option<&str>, cx: &mut App) {
    let resolved = resolve_otlp_endpoint(endpoint);
    let endpoint_redacted = redact_endpoint(&resolved);
    let enabled = consented && mode != TelemetryMode::Disabled;

    let (sink, connection_error): (Arc<dyn TelemetrySink>, Option<String>) =
        match (&mode, consented) {
            (TelemetryMode::Disabled, _) | (_, false) => (Arc::new(DisabledSink), None),
            (TelemetryMode::LocalOnly, true) => (Arc::new(LocalSink), None),
            (TelemetryMode::Remote, true) => {
                let sink = RemoteSink::new(&resolved);
                let err = if sink.connected {
                    None
                } else {
                    Some(format!("failed to connect OTLP exporter to {resolved}"))
                };
                (Arc::new(sink), err)
            }
        };

    let next = TelemetrySnapshot {
        compiled: true,
        consented,
        enabled,
        mode: mode.clone(),
        endpoint_redacted,
        events_recorded: snapshot(cx).events_recorded,
        last_export_error: connection_error,
        last_error: None,
    };

    tracing::info!(
        target: "gpui_starter::telemetry",
        consented = next.consented,
        enabled = next.enabled,
        mode = ?next.mode,
        endpoint = ?next.endpoint_redacted,
        "telemetry mode updated"
    );

    set_capability(&next, cx);
    cx.update_global::<TelemetrySnapshot, _>(|snap, _cx| {
        *snap = next;
    });
    cx.set_global(TelemetryRuntime { sink });
}

pub fn record_event(name: &str, cx: &mut App) {
    with_runtime(cx, |runtime, cx| {
        let result = runtime.sink.record_event(name);
        handle_record_result(result, cx);
    });
}

pub fn record_error(error: &str, cx: &mut App) {
    with_runtime(cx, |runtime, cx| {
        let result = runtime.sink.record_error(error);
        handle_record_result(result, cx);
    });
}

pub fn set_user_property(key: &str, value: &str, cx: &mut App) {
    with_runtime(cx, |runtime, cx| {
        let result = runtime.sink.set_user_properties(key, value);
        handle_record_result(result, cx);
    });
}

pub fn flush(cx: &mut App) {
    with_runtime(cx, |runtime, cx| {
        cx.update_global::<TelemetrySnapshot, _>(|snap, _cx| {
            if let Err(err) = runtime.sink.flush() {
                snap.last_export_error = Some(err.to_string());
                snap.last_error = Some(err.to_string());
            }
        });
    });
}

/// Flush pending telemetry and shut down the global tracer provider.
///
/// This is a **one-way** operation: after calling `shutdown` no further spans
/// can be exported. Use [`flush`] instead when you only need to push pending
/// spans to the collector without disabling telemetry.
///
/// Safe to call multiple times. Subsequent calls after the first are no-ops at
/// the OpenTelemetry level.
pub fn shutdown(cx: &mut App) {
    let state = snapshot(cx);
    tracing::debug!(
        target: "gpui_starter::telemetry",
        enabled = state.enabled,
        mode = ?state.mode,
        events_recorded = state.events_recorded,
        "telemetry shutdown requested"
    );
    // Flush pending spans through the sink (uses force_flush, not shutdown).
    flush(cx);
    // Now shut down the global tracer provider permanently. This is the only
    // place where shutdown_tracer_provider() should be called.
    global::shutdown_tracer_provider();
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Resolve the OTLP endpoint URL.
///
/// Precedence:
/// 1. Explicit `endpoint` argument passed by the caller.
/// 2. `OTEL_EXPORTER_OTLP_ENDPOINT` environment variable.
/// 3. [`DEFAULT_OTLP_ENDPOINT`] fallback (`http://localhost:4318`).
fn resolve_otlp_endpoint(explicit: Option<&str>) -> String {
    if let Some(ep) = explicit
        && !ep.trim().is_empty()
    {
        return ep.trim().to_owned();
    }
    match std::env::var(ENV_OTLP_ENDPOINT) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_owned(),
        _ => DEFAULT_OTLP_ENDPOINT.to_owned(),
    }
}

fn with_runtime(cx: &mut App, f: impl FnOnce(TelemetryRuntime, &mut App)) {
    if let Some(runtime) = cx.try_global::<TelemetryRuntime>().cloned() {
        f(runtime, cx);
    }
}

fn handle_record_result(result: Result<(), TelemetryError>, cx: &mut App) {
    cx.update_global::<TelemetrySnapshot, _>(|snap, _cx| match result {
        Ok(()) => {
            snap.events_recorded = snap.events_recorded.saturating_add(1);
            snap.last_error = None;
        }
        Err(err) => {
            snap.last_export_error = Some(err.to_string());
            snap.last_error = Some(err.to_string());
        }
    });
}

fn set_capability(snapshot: &TelemetrySnapshot, cx: &mut App) {
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
}

fn redact_endpoint(endpoint: &str) -> Option<String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        return None;
    }
    let host = endpoint
        .split("://")
        .nth(1)
        .unwrap_or(endpoint)
        .split('/')
        .next()
        .unwrap_or(endpoint);
    Some(format!("{host}/…"))
}

#[cfg(test)]
#[path = "../telemetry.test.rs"]
mod telemetry_test;
