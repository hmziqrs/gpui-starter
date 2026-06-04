---
title: "WebSocket Support"
description: "Optional WebSocket feature for real-time data streaming in your GPUI desktop app."
---

The WebSocket module (`src/platform/network/websocket.rs`) provides an async client with automatic reconnection, exponential backoff, and a pending message buffer. It is feature-gated so the binary compiles cleanly without it. When you need real-time data (collaboration, live feeds, device telemetry), flip the feature on and connect.

For the broader network layer, see [Architecture](/docs/architecture/). For async patterns in GPUI, see the [state management](/blog/rust-desktop-state-management/) blog post.

## Enabling the feature

The client depends on `tokio-tungstenite` with native TLS. Both are optional, gated behind the `websocket` feature flag. Add to `Cargo.toml`:

```toml
[dependencies]
tokio-tungstenite = { version = "0.24", optional = true, features = ["native-tls"] }
tokio-stream = { version = "0.1", optional = true }
futures-util = "0.3"

[features]
websocket = ["dep:tokio-tungstenite", "dep:tokio-stream"]
```

Then build with the feature enabled:

```bash
cargo run --features websocket
```

When the feature is off, a stub `WebSocketClient` compiles instead. It has the same method signatures but returns `WebSocketError::FeatureDisabled` on every call, so code that references the client compiles regardless of the feature flag.

## ConnectionState

The client tracks its status through a state machine:

```rust
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u8 },
    Closed,
}
```

The transitions are linear: `Disconnected` to `Connecting` on the first attempt, then `Connected` when the handshake completes. If the stream ends, back to `Disconnected`, then `Reconnecting { attempt }` for each retry. When `max_retries` is exceeded, the state moves to `Closed`. Read `client.state` at any time to drive connection status indicators in your UI.

## Creating a client

```rust
use crate::websocket::{WebSocketClient, ReconnectPolicy, MessageHandler};

let client = WebSocketClient::new("wss://example.com/ws".to_string());
```

The builder pattern lets you set a custom reconnection policy and message handler:

```rust
let policy = ReconnectPolicy {
    max_retries: 10,
    base_delay_ms: 1000,
    max_delay_ms: Some(60_000),
};

let handler: MessageHandler = Box::new(|text: &str| {
    tracing::info!(target: "app", "received: {text}");
});

let client = WebSocketClient::new("wss://example.com/ws".to_string())
    .with_reconnect_policy(policy)
    .on_message(handler);
```

`MessageHandler` is a type alias for `Box<dyn Fn(&str) + Send + Sync>`. The callback receives text messages only. Binary, ping, and pong frames are ignored by the read loop.

## ReconnectPolicy

Controls how the client behaves when the connection drops:

```rust
pub struct ReconnectPolicy {
    pub max_retries: u8,
    pub base_delay_ms: u64,
    pub max_delay_ms: Option<u64>,
}
```

| Field | Default | Purpose |
|-------|---------|---------|
| `max_retries` | `5` | Total reconnection attempts before the client gives up and enters `Closed` state |
| `base_delay_ms` | `500` | Starting delay in milliseconds for exponential backoff |
| `max_delay_ms` | `Some(30_000)` | Optional upper bound so backoff does not grow indefinitely |

The delay formula is `base_delay_ms * 2^attempt`, capped at `max_delay_ms`. For the default policy the sequence is roughly 500ms, 1s, 2s, 4s, 8s before the client stops trying.

## Connection lifecycle

Call `connect_loop()` to start the main loop. This method connects, reads incoming messages until the stream closes, then retries according to the reconnection policy:

```rust
if let Err(e) = client.connect_loop().await {
    tracing::error!("websocket terminated: {e}");
}
```

Inside the loop: state is set to `Connecting` (or `Reconnecting { attempt }` for retries), `connect_async` is called, and on success the stream is split into write and read halves. The write half is stored in an `Arc<Mutex<Option<WriteHalf>>>` so `send()` can access it. Any pending messages accumulated during disconnection are flushed before the read loop begins. The read loop calls `on_message` for each text frame. When the stream ends or errors, state returns to `Disconnected` and the next attempt starts. If `max_retries` is exceeded, state becomes `Closed` and an error is returned.

## Sending messages

```rust
client.send("hello").await?;
```

Three things can happen:

1. When connected, the message is written to the WebSocket write half immediately.
2. When disconnected or reconnecting, the message is pushed into a pending buffer (up to 64 messages). When the next connection is established, `flush_pending` drains the buffer into the fresh write half.
3. If the buffer is full (64 messages already pending), `send` returns `WebSocketError::NotConnected` and the message is dropped.

The pending buffer prevents message loss during brief disconnections. It is not a durable queue. If the client enters `Closed` state or you call `close()`, the buffer is cleared.

## Closing

```rust
client.close().await?;
```

This sends a close frame, sets state to `Closed`, and clears the pending buffer. The client will not reconnect after `close()`.

## GPUI integration

The module provides `spawn_connect` for wiring into GPUI's async system:

```rust
use crate::websocket;

// During app init or in response to a user action:
let url = "wss://example.com/ws".to_string();
websocket::spawn_connect(url, cx);
```

`spawn_connect` creates a `WebSocketClient`, wraps `connect_loop` in `cx.spawn`, and detaches the task. Logging is routed through the `gpui_starter::websocket` target.

For more control (custom message handler, reconnect policy), spawn manually:

```rust
use crate::websocket::{WebSocketClient, ReconnectPolicy};

let url = "wss://example.com/ws".to_string();
cx.spawn(async move |cx| {
    let mut client = WebSocketClient::new(url)
        .with_reconnect_policy(ReconnectPolicy {
            max_retries: 10,
            base_delay_ms: 1000,
            max_delay_ms: Some(30_000),
        })
        .on_message(Box::new(|text| {
            tracing::info!("ws message: {text}");
        }));

    cx.background_executor()
        .spawn(async move {
            if let Err(e) = client.connect_loop().await {
                tracing::error!("websocket error: {e}");
            }
        })
        .await
}).detach();
```

This follows the same pattern used by `connectivity::check_now` and `tasks::start_demo_task` in gpui-starter: `cx.spawn` creates a GPUI-managed async context, and the I/O work runs on the background executor.

## WebSocketError

```rust
pub enum WebSocketError {
    Connection(String),
    Send(tokio_tungstenite::tungstenite::Error),  // feature-gated
    Close(tokio_tungstenite::tungstenite::Error),  // feature-gated
    NotConnected,
    FeatureDisabled,
}
```

| Variant | When it occurs |
|---------|---------------|
| `Connection` | `max_retries` exceeded, connection URL invalid, TLS handshake failed |
| `Send` | Writing to the WebSocket stream failed |
| `Close` | Sending the close frame failed |
| `NotConnected` | `send()` called while disconnected and the pending buffer is full |
| `FeatureDisabled` | Any method called when the `websocket` feature is off |

## Configuration summary

All configuration is done through Rust code at client construction time. There are no config file entries or environment variables.

| Setting | Default | Where to set |
|---------|---------|-------------|
| WebSocket URL | (required) | `WebSocketClient::new(url)` |
| Max retries | `5` | `ReconnectPolicy::max_retries` |
| Base delay | `500ms` | `ReconnectPolicy::base_delay_ms` |
| Max delay | `30s` | `ReconnectPolicy::max_delay_ms` |
| Message handler | `None` | `.on_message(handler)` |
| Pending buffer size | `64` | Hardcoded (`MAX_PENDING_MESSAGES`) |

## Logging

All log statements use the `gpui_starter::websocket` target. Enable trace-level logging to see connection attempts, reconnection delays, and pending buffer flushes:

```
RUST_LOG=gpui_starter::websocket=trace cargo run --features websocket
```

For general debugging techniques including log filtering, see the [debugging techniques](/blog/rust-desktop-debugging-techniques/) blog post.

## Testing

The reconnect policy and connection state machine have unit tests in `src/platform/network/websocket.test.rs`. These tests run without the `websocket` feature enabled because they only exercise the types and the stub client.

Test cases:

- `reconnect_delay_increases_exponentially`: verifies backoff growth across attempts.
- `reconnect_delay_respects_cap`: verifies `max_delay_ms` clamps the delay.
- `stub_client_returns_error_without_feature`: verifies the stub surfaces `FeatureDisabled`.

For integration testing with a real server, see [Testing](/docs/testing/) for the GPUI test harness setup. You can point the client at a local test server and assert on the message handler callback.
