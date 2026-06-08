//! WebSocket client scaffold.
//!
//! To use, add to `Cargo.toml`:
//! ```toml
//! tokio-tungstenite = { version = "0.24", features = ["native-tls"] }
//! ```
//!
//! Then enable the `websocket` feature in your build:
//! ```toml
//! [features]
//! websocket = ["dep:tokio-tungstenite"]
//! ```
//!
//! Integration with GPUI context uses `cx.spawn()` for async operations,
//! following the same pattern as `connectivity::check_now` and `tasks::start_demo_task`.

#![allow(dead_code)]

mod client;
mod types;

pub use types::{ConnectionState, MessageHandler, ReconnectPolicy, WebSocketError};
pub use client::WebSocketClient;

// ---------------------------------------------------------------------------
// GPUI integration helpers (always compiled).
// ---------------------------------------------------------------------------

/// Spawn a WebSocket client connect loop from GPUI context.
///
/// ```ignore
/// // In your app init or action handler:
/// let url = "wss://example.com/ws".to_string();
/// websocket::spawn_connect(url, cx);
/// ```
///
/// This mirrors the pattern in `connectivity::check_now` and `tasks::start_demo_task`:
/// `cx.spawn` creates a GPUI-managed async context, and the heavy I/O is
/// delegated to `background_executor().spawn(...)`.
#[cfg(feature = "websocket")]
pub fn spawn_connect(url: String, cx: &mut gpui::App) {
    let mut client = WebSocketClient::new(url);
    cx.spawn(async move |cx| {
        cx.background_executor()
            .spawn(async move {
                if let Err(e) = client.connect_loop().await {
                    tracing::error!(
                        target: "gpui_starter::websocket",
                        error = %e,
                        "websocket connect loop terminated with error"
                    );
                }
            })
            .await
    })
    .detach();
}

// ---------------------------------------------------------------------------
// Tests (always compiled, no dependency required).
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "../websocket.test.rs"]
mod websocket_test;
