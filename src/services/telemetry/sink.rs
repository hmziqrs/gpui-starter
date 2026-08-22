//! Telemetry sink implementations: disabled, local, and remote (OTLP).

use super::TelemetryError;

// ---------------------------------------------------------------------------
// OTLP exporter (feature-gated)
// ---------------------------------------------------------------------------

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
pub(super) fn install_otlp_tracer(
    endpoint: &str,
) -> Result<opentelemetry_sdk::trace::TracerProvider, TelemetryError> {
    use opentelemetry::KeyValue;
    use opentelemetry::global;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::Resource;
    use opentelemetry_sdk::propagation::TraceContextPropagator;
    use opentelemetry_sdk::runtime::Tokio;

    use super::SERVICE_NAME;

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
pub(super) fn install_otlp_tracer(endpoint: &str) -> Result<(), TelemetryError> {
    tracing::debug!(
        target: "gpui_starter::telemetry",
        endpoint = %endpoint,
        "OTLP export skipped (otlp feature disabled); endpoint noted for future use"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// DisabledSink
// ---------------------------------------------------------------------------

#[derive(Default)]
pub(super) struct DisabledSink;

impl super::TelemetrySink for DisabledSink {
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

// ---------------------------------------------------------------------------
// LocalSink
// ---------------------------------------------------------------------------

#[derive(Default)]
pub(super) struct LocalSink;

impl super::TelemetrySink for LocalSink {
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

// ---------------------------------------------------------------------------
// RemoteSink
// ---------------------------------------------------------------------------

/// Remote telemetry sink that exports spans via the OTLP protocol over HTTP.
///
/// # Configuration
///
/// The collector endpoint is resolved in this order:
///
/// 1. The `endpoint` argument passed to [`set_mode`](super::set_mode).
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
/// [`flush`](super::TelemetrySink::flush) pushes pending spans to the collector
/// **without** disabling the tracer provider. The provider remains active and
/// continues accepting new spans after a flush.
///
/// [`shutdown`](crate::services::telemetry::shutdown) terminates the provider
/// permanently. No further spans can be exported after shutdown.
#[derive(Clone)]
pub(super) struct RemoteSink {
    endpoint: String,
    pub(super) connected: bool,
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
    pub(super) fn new(endpoint: &str) -> Self {
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
    fn install(endpoint: &str) -> (bool, Option<opentelemetry_sdk::trace::TracerProvider>) {
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
            Err(TelemetryError::Otlp(Box::new(std::io::Error::other(
                format!("force_flush errors: {msg}"),
            ))))
        }
    }

    /// No-op flush path when the `otlp` feature is disabled.
    #[cfg(not(feature = "otlp"))]
    fn force_flush_provider(&self) -> Result<(), TelemetryError> {
        Ok(())
    }

    /// Guard for sink methods that require collector connectivity.
    ///
    /// When the sink is connected, returns `Ok(())` so the caller proceeds with
    /// its normal queue/flush path. When it is not connected, invokes the
    /// supplied `log_dropped` closure (so each caller can emit its own richly
    /// structured "dropped"/"skipped" line preserving per-call fields such as
    /// the event name, error text, or user property key/value) and returns
    /// [`TelemetryError::NotAvailable`].
    ///
    /// The structured log emission deliberately lives at each call site rather
    /// than inside this helper: `tracing` field sets are baked into the macro
    /// at the call site and cannot be threaded through a single shared
    /// invocation without dropping the per-call context that is the whole
    /// point of the log.
    fn require_connected(&self, log_dropped: impl FnOnce()) -> Result<(), TelemetryError> {
        if self.connected {
            return Ok(());
        }
        log_dropped();
        Err(TelemetryError::NotAvailable(format!(
            "OTLP exporter not connected to {}",
            self.endpoint
        )))
    }
}

impl super::TelemetrySink for RemoteSink {
    fn record_event(&self, name: &str) -> Result<(), TelemetryError> {
        self.require_connected(|| {
            tracing::warn!(
                target: "gpui_starter::telemetry",
                endpoint = %self.endpoint,
                event = %name,
                "remote telemetry event dropped (not connected)"
            );
        })?;
        tracing::debug!(target: "gpui_starter::telemetry", endpoint = %self.endpoint, event = %name, "remote telemetry event queued");
        Ok(())
    }

    fn record_error(&self, error: &str) -> Result<(), TelemetryError> {
        self.require_connected(|| {
            tracing::warn!(
                target: "gpui_starter::telemetry",
                endpoint = %self.endpoint,
                error = %error,
                "remote telemetry error dropped (not connected)"
            );
        })?;
        tracing::warn!(target: "gpui_starter::telemetry", endpoint = %self.endpoint, error = %error, "remote telemetry error queued");
        Ok(())
    }

    fn set_user_properties(&self, key: &str, value: &str) -> Result<(), TelemetryError> {
        self.require_connected(|| {
            tracing::debug!(
                target: "gpui_starter::telemetry",
                endpoint = %self.endpoint,
                key = %key,
                value = %value,
                "remote telemetry user property dropped (not connected)"
            );
        })?;
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
        self.require_connected(|| {
            tracing::debug!(
                target: "gpui_starter::telemetry",
                endpoint = %self.endpoint,
                "remote telemetry flush skipped (not connected)"
            );
        })?;
        tracing::debug!(target: "gpui_starter::telemetry", endpoint = %self.endpoint, "remote telemetry flush");
        self.force_flush_provider()
    }
}
