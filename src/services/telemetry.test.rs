use super::*;
use std::sync::{Arc, Mutex};

struct FakeSink {
    events: Arc<Mutex<Vec<String>>>,
}

impl TelemetrySink for FakeSink {
    fn record_event(&self, name: &str) -> Result<(), TelemetryError> {
        self.events.lock().expect("lock").push(name.to_string());
        Ok(())
    }

    fn record_error(&self, error: &str) -> Result<(), TelemetryError> {
        self.events
            .lock()
            .expect("lock")
            .push(format!("err:{error}"));
        Ok(())
    }

    fn set_user_properties(&self, key: &str, value: &str) -> Result<(), TelemetryError> {
        self.events
            .lock()
            .expect("lock")
            .push(format!("prop:{key}={value}"));
        Ok(())
    }

    fn flush(&self) -> Result<(), TelemetryError> {
        Ok(())
    }
}

#[test]
fn redacts_endpoint_host_only() {
    let value =
        redact_endpoint("https://telemetry.example.com/v1/events").expect("redacted endpoint");
    assert_eq!(value, "telemetry.example.com/…");
}

#[test]
fn fake_sink_records_calls() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = FakeSink {
        events: events.clone(),
    };
    sink.record_event("evt").expect("event");
    sink.record_error("oops").expect("error");
    sink.set_user_properties("k", "v").expect("prop");

    let data = events.lock().expect("lock").clone();
    assert_eq!(data, vec!["evt", "err:oops", "prop:k=v"]);
}

#[test]
fn resolve_endpoint_prefers_explicit() {
    let ep = resolve_otlp_endpoint(Some("https://custom.example.com:4318"));
    assert_eq!(ep, "https://custom.example.com:4318");
}

#[test]
fn resolve_endpoint_ignores_empty_explicit() {
    let ep = resolve_otlp_endpoint(Some(""));
    // Should fall through to env var or default -- at minimum must not be empty.
    assert!(!ep.is_empty());
}

#[test]
fn resolve_endpoint_falls_back_to_default() {
    // Clear the env var so the default is guaranteed.
    // SAFETY: This test does not read the env var concurrently; the
    // removal is scoped to this single-threaded test function.
    unsafe { std::env::remove_var(ENV_OTLP_ENDPOINT) };
    let ep = resolve_otlp_endpoint(None);
    assert_eq!(ep, DEFAULT_OTLP_ENDPOINT);
}

#[test]
fn remote_sink_connected_without_feature() {
    // Without the `otlp` feature, install_otlp_tracer is a no-op success,
    // so connected should be true (tracing-only path).
    let sink = RemoteSink::new("http://localhost:9999");
    assert!(sink.connected);
}
