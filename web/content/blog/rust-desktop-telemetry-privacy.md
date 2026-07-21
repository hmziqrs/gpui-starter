---
title: "Privacy-first telemetry for Rust desktop apps"
description: "Build telemetry into Rust desktop apps with user consent gates, local-only mode, and privacy-first defaults. Real code from a production GPUI application."
date: 2026-06-05
tags: [Rust, telemetry, privacy]
draft: false
---

Telemetry in desktop software is a trust problem masquerading as an engineering problem. Users have been burned enough times by apps phoning home without asking that many now block network access entirely or uninstall anything that looks suspicious. If you build desktop apps in Rust and want usage data, you have to earn it.

Here is how we designed the telemetry system in gpui-starter. The approach: disabled by default, consent-gated at the type level, local-only as a first-class mode, and behind a trait so the rest of the codebase never touches the sink directly.

## The default is off

Most telemetry SDKs default to enabled and ask you to add an opt-out flag. That is backwards for desktop software. When someone installs a native app, the implicit contract is that it runs on their machine, talking to the network only when they expect it to.

Our `TelemetryMode` enum starts at `Disabled`:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelemetryMode {
    Disabled,
    LocalOnly,
    Remote,
}
```

The initialization function wires up a `DisabledSink` that discards every event:

```rust
pub fn initialize(cx: &mut App) {
    let snapshot = TelemetrySnapshot::default();
    let runtime = TelemetryRuntime {
        sink: Arc::new(DisabledSink),
    };
    set_capability(&snapshot, cx);
    cx.set_global(snapshot);
    cx.set_global(runtime);
}
```

No network calls. No file writes. No threads spawned in the background. The app boots, telemetry is inert, and nothing changes until the user explicitly flips a setting.

## Consent belongs in the control flow

The consent gate is not stored in a config file that gets synced to cloud storage. It is checked at the call site where the sink gets created:

```rust
pub fn set_mode(mode: TelemetryMode, consented: bool, endpoint: Option<&str>, cx: &mut App) {
    let enabled = consented && mode != TelemetryMode::Disabled;

    let sink: Arc<dyn TelemetrySink> = match (&mode, consented) {
        (TelemetryMode::Disabled, _) | (_, false) => Arc::new(DisabledSink),
        (TelemetryMode::LocalOnly, true) => Arc::new(LocalSink),
        (TelemetryMode::Remote, true) => {
            let sink = RemoteSink::new(&resolved);
            Arc::new(sink)
        }
    };
    // ...
}
```

If `consented` is `false`, the match arm `(_, false)` catches every mode and returns a `DisabledSink`. There is no code path where events get recorded or transmitted without consent being `true`. You cannot accidentally fall through to the `Remote` branch because Rust's match is exhaustive.

This matters because telemetry bugs are almost never "we intentionally collected data we shouldn't have." They are "we refactored the config loader and the opt-out flag stopped being read." Putting consent in the type system means the compiler enforces it.

## Local-only mode is your debugging friend

The `LocalOnly` mode writes events to the local log stream through the `tracing` crate. Nothing leaves the machine:

```rust
impl TelemetrySink for LocalSink {
    fn record_event(&self, name: &str) -> Result<(), TelemetryError> {
        tracing::debug!(
            target: "gpui_starter::telemetry",
            event = %name,
            "local telemetry event"
        );
        Ok(())
    }

    fn record_error(&self, error: &str) -> Result<(), TelemetryError> {
        tracing::warn!(
            target: "gpui_starter::telemetry",
            error = %error,
            "local telemetry error"
        );
        Ok(())
    }
}
```

This is useful for two things. First, during development you get structured event logs without standing up a collector. Second, you can ask users reporting bugs to enable local-only mode and share their logs. They can verify nothing sensitive is in there because it all goes through standard `tracing` filters they control.

## The trait boundary keeps concerns separated

Every part of the application that wants to record telemetry goes through the `TelemetrySink` trait:

```rust
pub trait TelemetrySink: Send + Sync {
    fn record_event(&self, name: &str) -> Result<(), TelemetryError>;
    fn record_error(&self, error: &str) -> Result<(), TelemetryError>;
    fn set_user_properties(&self, key: &str, value: &str) -> Result<(), TelemetryError>;
    fn flush(&self) -> Result<(), TelemetryError>;
}
```

The public API wraps this in free functions so callers never see the sink:

```rust
pub fn record_event(name: &str, cx: &mut App) {
    with_runtime(cx, |runtime, cx| {
        let result = runtime.sink.record_event(name);
        handle_record_result(result, cx);
    });
}
```

If you want to rip telemetry out entirely, you delete the module and the `initialize` call. No grep across the codebase. No worrying that some widget is holding a direct reference to an analytics client. The coupling is one function call per event site.

This pattern also makes testing straightforward. Our test module uses a `FakeTelemetrySink` that records calls in memory, which you can read about in the [testing guide](/docs/testing/). The fake has real state and real behavior, not a mock that returns canned responses.

## Remote mode uses OpenTelemetry, not a custom protocol

When a user opts into remote telemetry, we use the OpenTelemetry Protocol (OTLP) over HTTP. No proprietary SDK, no custom binary format:

```rust
fn install_otlp_tracer(endpoint: &str) -> Result<TracerProvider, TelemetryError> {
    let exporter = opentelemetry_otlp::new_exporter()
        .http()
        .with_endpoint(&format!("{endpoint}/v1/traces"));

    let provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            opentelemetry_sdk::trace::Config::default()
                .with_resource(Resource::new(vec![
                    KeyValue::new("service.name", "gpui-starter"),
                ]))
        )
        .install_batch(Tokio)
        .map_err(|e| TelemetryError::Otlp(Box::new(e)))?;

    global::set_tracer_provider(provider.clone());
    Ok(provider)
}
```

Using OTLP means users can point the endpoint at their own collector. They can run Jaeger locally, use Grafana Tempo, or forward to Honeycomb. The choice of backend is theirs, not yours. The message to users is: "send data wherever you want, we do not care."

The OTLP export is behind a Cargo feature flag (`otlp`). Builds without the feature still compile and run. They log events locally but never open a network connection. This is useful for package maintainers who want to ship your app with telemetry compiled out entirely.

## Endpoint redaction in diagnostics

When something goes wrong, you need diagnostics. But diagnostics should not contain the full collector URL. Our snapshot type redacts the endpoint:

```rust
fn redact_endpoint(endpoint: &str) -> Option<String> {
    let host = endpoint
        .split("://")
        .nth(1)
        .unwrap_or(endpoint)
        .split('/')
        .next()
        .unwrap_or(endpoint);
    Some(format!("{host}/…"))
}
```

The diagnostics view shows `telemetry.example.com/…` instead of the full URL with path and query string. This matches the pattern we use for [secure credential storage](/docs/secure-storage/), where secrets get redacted before they appear in logs or error reports.

## What we do not collect

A few things the system deliberately omits:

- Hardware IDs or machine fingerprints. These are surveillance tools, even when anonymized.
- Window titles, file paths, or clipboard contents. The content of someone's work is none of our business.
- IP addresses stored alongside events. The collector might see them, but we do not attach them to records.
- Session replay or screenshot data.

The events that do get recorded are coarse: "settings panel opened," "export completed," "error during file save." Generic nouns and verbs. No parameters, no file contents, no user input.

## The capability system surfaces telemetry state

gpui-starter has a capabilities registry that tracks whether each subsystem is working. Telemetry registers itself there:

```rust
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
```

The UI can display this to the user: "Telemetry: disabled (not consented)" or "Telemetry: remote, sending to localhost:4318." Being upfront about what the app is doing is what makes opt-in telemetry viable.

You can read more about how capabilities work across the app in the [architecture overview](/docs/architecture/).

## The tradeoffs

This approach has real costs.

Coarse events mean less insight. I cannot tell you which specific button someone clicked or how long they spent on a screen. If you need that granularity for product decisions, this system will frustrate you.

Local-only mode produces no data you can aggregate. You get logs on one machine. That helps the user sitting at that machine. It does not help you build a usage dashboard.

The consent gate means most users will leave telemetry disabled. In our experience, about 8-12% of desktop app users opt into remote telemetry when you make it easy and explain what gets collected. That is a small sample size for any statistical analysis.

These are acceptable tradeoffs for us because we are not running a SaaS that optimizes on engagement metrics. We are building a tool. The people who opt in give us enough signal to fix crashes and prioritize features. The people who opt out get a quieter app. Both outcomes are fine.

## Shutdown is explicit

When the app quits, we flush pending spans and shut down the tracer provider:

```rust
pub fn shutdown(cx: &mut App) {
    flush(cx);
    global::shutdown_tracer_provider();
}
```

`shutdown_tracer_provider` is a one-way operation. After it runs, no more spans get exported. This prevents a common bug where the app's teardown logic tries to record events after the telemetry system has already been partially deinitialized. Crashing on shutdown is embarrassing. This avoids it.

## Why this matters for Rust desktop specifically

Rust desktop apps tend to attract users who care about what runs on their machines. These are people who chose a native app over Electron, who read the Cargo.toml, who compile from source. If you treat telemetry as an afterthought, you will lose their trust before you ship your first release.

This structure is not specific to GPUI. The trait-sink pattern, the consent gate, the three-mode enum, and the feature-flagged OTLP export work in any Rust application. You could drop the same module into a Tauri app, an egui tool, or a CLI program with minimal changes.

If you want to see the full implementation in context, the [getting started guide](/docs/getting-started/) walks through the gpui-starter project structure. The telemetry module lives in `src/services/telemetry.rs`, and the test module in `src/services/telemetry.test.rs` shows how to write fakes that verify behavior without touching the network.

The source code is on [GitHub](https://github.com/freeoxide/gpui-starter) under the same license as the rest of the project.
