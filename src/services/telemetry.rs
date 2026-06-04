use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

use gpui::{App, BorrowAppContext as _, Global};
use opentelemetry::global;
use tracing_opentelemetry as _;

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

#[derive(Default)]
struct DisabledSink;

impl TelemetrySink for DisabledSink {
    fn record_event(&self, _name: &str) -> Result<(), TelemetryError> {
        Ok(())
    }

    fn record_error(&self, _error: &str) -> Result<(), TelemetryError> {
        Ok(())
    }

    fn set_user_properties(&self, _key: &str, _value: &str) -> Result<(), TelemetryError> {
        Ok(())
    }

    fn flush(&self) -> Result<(), TelemetryError> {
        Ok(())
    }
}

#[derive(Default)]
struct LocalSink;

impl TelemetrySink for LocalSink {
    fn record_event(&self, name: &str) -> Result<(), TelemetryError> {
        tracing::debug!(target: "gpui_starter::telemetry", event = %name, "local telemetry event");
        Ok(())
    }

    fn record_error(&self, error: &str) -> Result<(), TelemetryError> {
        tracing::warn!(target: "gpui_starter::telemetry", error = %error, "local telemetry error");
        Ok(())
    }

    fn set_user_properties(&self, key: &str, value: &str) -> Result<(), TelemetryError> {
        tracing::debug!(target: "gpui_starter::telemetry", key = %key, value = %value, "local telemetry user property");
        Ok(())
    }

    fn flush(&self) -> Result<(), TelemetryError> {
        Ok(())
    }
}

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

/// Attempt to install an OTLP HTTP tracer provider on the global
/// OpenTelemetry pipeline.
///
/// Returns `Ok(provider)` when the provider was installed successfully or when
/// the `otlp` feature is not enabled (no-op). Returns a human-readable
/// error string when the exporter cannot reach the collector.
///
/// The returned [`opentelemetry_sdk::trace::TracerProvider`] is kept by the
/// caller so it can invoke [`force_flush`](opentelemetry_sdk::trace::TracerProvider::force_flush)
/// without shutting down the global provider.
#[cfg(feature = "otlp")]
fn install_otlp_tracer(
    endpoint: &str,
) -> Result<opentelemetry_sdk::trace::TracerProvider, TelemetryError> {
    use opentelemetry::KeyValue;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::Resource;
    use opentelemetry_sdk::propagation::TraceContextPropagator;
    use opentelemetry_sdk::runtime::Tokio;

    let exporter = opentelemetry_otlp::new_exporter()
        .http()
        .with_endpoint(&format!("{endpoint}/v1/traces"));

    let provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(opentelemetry_sdk::trace::Config::default().with_resource(
            Resource::new(vec![KeyValue::new("service.name", SERVICE_NAME)]),
        ))
        .install_batch(Tokio)
        .map_err(|e| TelemetryError::Otlp(Box::new(e)))?;

    global::set_text_map_propagator(TraceContextPropagator::new());
    // Pass a clone to the global registry; keep the original for force_flush.
    global::set_tracer_provider(provider.clone());

    tracing::info!(
        target: "gpui_starter::telemetry",
        endpoint = %endpoint,
        "OTLP tracer provider installed"
    );
    Ok(provider)
}

/// No-op fallback when the `otlp` feature is disabled.
///
/// Logs the endpoint for diagnostics but does not create an exporter.
/// Returns a unit `()` since there is no provider to track.
#[cfg(not(feature = "otlp"))]
fn install_otlp_tracer(endpoint: &str) -> Result<(), TelemetryError> {
    tracing::debug!(
        target: "gpui_starter::telemetry",
        endpoint = %endpoint,
        "OTLP export skipped (otlp feature disabled); endpoint noted for future use"
    );
    Ok(())
}

/// Remote telemetry sink that exports spans via the OTLP protocol over HTTP.
///
/// # Configuration
///
/// The collector endpoint is resolved in this order:
///
/// 1. The `endpoint` argument passed to [`set_mode`].
/// 2. The `OTEL_EXPORTER_OTLP_ENDPOINT` environment variable.
/// 3. The built-in default `http://localhost:4318`.
///
/// # Prerequisites
///
/// Real export requires the `otlp` Cargo feature. Without it the sink still
/// records events through the tracing layer (visible via `tracing-subscriber`)
/// but does not ship them to a collector.
///
/// # Error handling
///
/// Connection failures during tracer installation are captured and surfaced
/// through [`TelemetrySnapshot::last_export_error`]. The sink itself never
/// panics; individual event records are logged at debug/warn level and
/// propagated to the subscriber regardless of collector reachability.
///
/// # Flush vs Shutdown
///
/// [`flush`](TelemetrySink::flush) pushes pending spans to the collector
/// **without** disabling the tracer provider. The provider remains active and
/// continues accepting new spans after a flush.
///
/// [`shutdown`](crate::services::telemetry::shutdown) terminates the provider
/// permanently. No further spans can be exported after shutdown.
#[derive(Clone)]
struct RemoteSink {
    endpoint: String,
    connected: bool,
    /// Handle to the SDK tracer provider, used to call `force_flush()` without
    /// shutting down the global provider. Only `Some` when the `otlp` feature
    /// is enabled and the provider was installed successfully.
    #[cfg(feature = "otlp")]
    provider: Option<opentelemetry_sdk::trace::TracerProvider>,
}

impl RemoteSink {
    /// Create a new `RemoteSink`, attempting to install the OTLP tracer
    /// provider in the process.
    ///
    /// The `connected` flag is set to `false` when installation fails, which
    /// allows callers to report the degradation through the capability system.
    #[allow(unused_variables)]
    fn new(endpoint: &str) -> Self {
        let (connected, provider) = Self::install(endpoint);
        Self {
            endpoint: endpoint.to_owned(),
            connected,
            #[cfg(feature = "otlp")]
            provider,
        }
    }

    /// Delegate to [`install_otlp_tracer`] and separate success/failure state.
    ///
    /// When the `otlp` feature is enabled, returns `(true, Some(provider))` on
    /// success. When the feature is disabled, returns `(true, None)` (the
    /// no-op path always succeeds).
    #[cfg(feature = "otlp")]
    fn install(
        endpoint: &str,
    ) -> (bool, Option<opentelemetry_sdk::trace::TracerProvider>) {
        match install_otlp_tracer(endpoint) {
            Ok(provider) => (true, Some(provider)),
            Err(err) => {
                tracing::warn!(
                    target: "gpui_starter::telemetry",
                    endpoint = %endpoint,
                    error = %err,
                    "OTLP tracer provider installation failed; events will be logged locally"
                );
                (false, None)
            }
        }
    }

    /// No-op install path when the `otlp` feature is disabled.
    #[cfg(not(feature = "otlp"))]
    fn install(endpoint: &str) -> (bool, ()) {
        let connected = match install_otlp_tracer(endpoint) {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(
                    target: "gpui_starter::telemetry",
                    endpoint = %endpoint,
                    error = %err,
                    "OTLP tracer provider installation failed; events will be logged locally"
                );
                false
            }
        };
        (connected, ())
    }

    /// Call `force_flush()` on the stored SDK tracer provider.
    ///
    /// This pushes all buffered spans to the collector without disabling the
    /// provider. Errors from individual span processors are collected and
    /// returned as a single [`TelemetryError::Otlp`].
    #[cfg(feature = "otlp")]
    fn force_flush_provider(&self) -> Result<(), TelemetryError> {
        let Some(provider) = self.provider.as_ref() else {
            tracing::warn!(
                target: "gpui_starter::telemetry",
                "force_flush called but no SDK provider is available; falling back to no-op"
            );
            return Ok(());
        };

        let results = provider.force_flush();
        let errors: Vec<_> = results.into_iter().filter_map(|r| r.err()).collect();

        if errors.is_empty() {
            Ok(())
        } else {
            let msg = errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            Err(TelemetryError::Otlp(Box::new(
                std::io::Error::other(format!("force_flush errors: {msg}")),
            )))
        }
    }

    /// No-op flush path when the `otlp` feature is disabled.
    #[cfg(not(feature = "otlp"))]
    fn force_flush_provider(&self) -> Result<(), TelemetryError> {
        Ok(())
    }
}

impl TelemetrySink for RemoteSink {
    fn record_event(&self, name: &str) -> Result<(), TelemetryError> {
        if !self.connected {
            tracing::warn!(
                target: "gpui_starter::telemetry",
                endpoint = %self.endpoint,
                event = %name,
                "remote telemetry event dropped (not connected)"
            );
            return Err(TelemetryError::NotAvailable(format!(
                "OTLP exporter not connected to {}",
                self.endpoint
            )));
        }
        tracing::debug!(target: "gpui_starter::telemetry", endpoint = %self.endpoint, event = %name, "remote telemetry event queued");
        Ok(())
    }

    fn record_error(&self, error: &str) -> Result<(), TelemetryError> {
        if !self.connected {
            tracing::warn!(
                target: "gpui_starter::telemetry",
                endpoint = %self.endpoint,
                error = %error,
                "remote telemetry error dropped (not connected)"
            );
            return Err(TelemetryError::NotAvailable(format!(
                "OTLP exporter not connected to {}",
                self.endpoint
            )));
        }
        tracing::warn!(target: "gpui_starter::telemetry", endpoint = %self.endpoint, error = %error, "remote telemetry error queued");
        Ok(())
    }

    fn set_user_properties(&self, key: &str, value: &str) -> Result<(), TelemetryError> {
        if !self.connected {
            tracing::debug!(
                target: "gpui_starter::telemetry",
                endpoint = %self.endpoint,
                key = %key,
                value = %value,
                "remote telemetry user property dropped (not connected)"
            );
            return Err(TelemetryError::NotAvailable(format!(
                "OTLP exporter not connected to {}",
                self.endpoint
            )));
        }
        tracing::debug!(target: "gpui_starter::telemetry", endpoint = %self.endpoint, key = %key, value = %value, "remote telemetry user property queued");
        Ok(())
    }

    /// Push pending spans to the collector **without** shutting down the provider.
    ///
    /// This is safe to call repeatedly (e.g. from the Settings "Flush
    /// Telemetry" button). The tracer provider remains fully operational after
    /// each flush, unlike `shutdown_tracer_provider()` which is a one-way
    /// destructive operation.
    fn flush(&self) -> Result<(), TelemetryError> {
        if !self.connected {
            tracing::debug!(
                target: "gpui_starter::telemetry",
                endpoint = %self.endpoint,
                "remote telemetry flush skipped (not connected)"
            );
            return Err(TelemetryError::NotAvailable(format!(
                "OTLP exporter not connected to {}",
                self.endpoint
            )));
        }
        tracing::debug!(target: "gpui_starter::telemetry", endpoint = %self.endpoint, "remote telemetry flush");
        self.force_flush_provider()
    }
}

#[derive(Clone)]
struct TelemetryRuntime {
    sink: Arc<dyn TelemetrySink>,
}

impl Global for TelemetryRuntime {}

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
#[path = "telemetry.test.rs"]
mod telemetry_test;
