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

#[derive(Debug, thiserror::Error)]
pub enum WebSocketError {
    #[error("connection failed: {0}")]
    Connection(String),
    #[cfg(feature = "websocket")]
    #[error("send failed: {0}")]
    Send(#[source] tokio_tungstenite::tungstenite::Error),
    #[cfg(feature = "websocket")]
    #[error("close failed: {0}")]
    Close(#[source] tokio_tungstenite::tungstenite::Error),
    #[error("not connected")]
    NotConnected,
    #[error("websocket feature not enabled")]
    FeatureDisabled,
}

// ---------------------------------------------------------------------------
// When the `websocket` feature is enabled, pull in the real dependency.
// Everything below compiles without it when the feature is off.
// ---------------------------------------------------------------------------
#[cfg(feature = "websocket")]
use tokio_tungstenite::tungstenite::Message;

// ---------------------------------------------------------------------------
// Types that are always available (no dependency required).
// ---------------------------------------------------------------------------

/// Connection state machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u8 },
    Closed,
}

/// Callback signature for incoming WebSocket messages.
///
/// Use this to wire message handling into GPUI context:
/// ```ignore
/// let handler: MessageHandler = Box::new(|text: &str| {
///     // Parse and dispatch into app state
/// });
/// ```
pub type MessageHandler = Box<dyn Fn(&str) + Send + Sync>;

/// Configuration for reconnection behaviour.
#[derive(Clone, Debug)]
pub struct ReconnectPolicy {
    /// Maximum number of reconnection attempts before giving up.
    pub max_retries: u8,
    /// Base delay for exponential backoff (milliseconds).
    pub base_delay_ms: u64,
    /// Optional cap so backoff does not grow indefinitely (milliseconds).
    pub max_delay_ms: Option<u64>,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            max_retries: 5,
            base_delay_ms: 500,
            max_delay_ms: Some(30_000),
        }
    }
}

impl ReconnectPolicy {
    /// Returns the delay for a given attempt number (0-indexed).
    ///
    /// Formula: `base_delay_ms * 2^attempt`, capped at `max_delay_ms`.
    pub fn delay_for_attempt(&self, attempt: u8) -> std::time::Duration {
        let exp = 1u64.checked_shl(attempt as u32).unwrap_or(u64::MAX);
        let raw = self.base_delay_ms.saturating_mul(exp);
        let capped = self.max_delay_ms.map_or(raw, |cap| raw.min(cap));
        std::time::Duration::from_millis(capped)
    }
}

// ---------------------------------------------------------------------------
// Feature-gated implementation (requires tokio-tungstenite).
// ---------------------------------------------------------------------------

#[cfg(feature = "websocket")]
mod live {
    use super::*;
    use futures_util::stream::SplitSink;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tokio_tungstenite::{connect_async, tungstenite::protocol::CloseFrame};

    /// The underlying TCP+TLS stream type used by `tokio-tungstenite`.
    type WsStream = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    /// Write half produced by `WebSocketStream::split()`.
    ///
    /// Stored inside an `Arc<Mutex<Option<...>>>` so that [`WebSocketClient::send`]
    /// and [`WebSocketClient::close`] can access it from any async task.
    type WriteHalf = SplitSink<WsStream, Message>;

    /// Shared inner state behind a `tokio::sync::Mutex`.
    ///
    /// The mutex guards the write half of the active WebSocket connection.
    /// It is `None` when disconnected and `Some(write_half)` when connected.
    type InnerSink = Arc<Mutex<Option<WriteHalf>>>;

    /// Small buffer for messages submitted via [`WebSocketClient::send`]
    /// while the connection is temporarily down. These are flushed into the
    /// write half as soon as the next connection is established.
    ///
    /// The buffer is bounded to [`MAX_PENDING_MESSAGES`] to avoid unbounded
    /// memory growth if the connection stays down for a long time.
    const MAX_PENDING_MESSAGES: usize = 64;

    /// Minimal WebSocket client with automatic reconnection.
    ///
    /// # GPUI integration
    ///
    /// Spawn the connect loop from a GPUI context:
    /// ```ignore
    /// let client = WebSocketClient::new("wss://example.com/ws".into());
    /// let inner = client.inner.clone();
    /// cx.spawn(async move |cx| {
    ///     cx.background_executor()
    ///         .spawn(client.connect_loop())
    ///         .await
    ///         .ok();
    /// }).detach();
    /// ```
    ///
    /// # Send / reconnect flow
    ///
    /// Messages sent while the socket is reconnecting are buffered in
    /// `pending` (up to [`MAX_PENDING_MESSAGES`]). When a new connection is
    /// established, the buffer is drained into the fresh write half before
    /// the read loop begins.
    pub struct WebSocketClient {
        pub url: String,
        pub state: ConnectionState,
        pub reconnect: ReconnectPolicy,
        pub on_message: Option<MessageHandler>,
        /// Write half of the active WebSocket, or `None` when disconnected.
        inner: InnerSink,
        /// Outbound messages queued while the connection is down.
        pending: Arc<Mutex<Vec<String>>>,
    }

    impl WebSocketClient {
        /// Create a new client targeting the given WebSocket URL.
        pub fn new(url: String) -> Self {
            Self {
                url,
                state: ConnectionState::Disconnected,
                reconnect: ReconnectPolicy::default(),
                on_message: None,
                inner: Arc::new(Mutex::new(None)),
                pending: Arc::new(Mutex::new(Vec::new())),
            }
        }

        /// Set a custom reconnection policy.
        pub fn with_reconnect_policy(mut self, policy: ReconnectPolicy) -> Self {
            self.reconnect = policy;
            self
        }

        /// Set a message handler callback.
        pub fn on_message(mut self, handler: MessageHandler) -> Self {
            self.on_message = Some(handler);
            self
        }

        /// Drain the pending buffer into the write half of the socket.
        ///
        /// Called immediately after a new connection is established so that
        /// messages queued during reconnection are delivered promptly.
        async fn flush_pending(&self, sink: &mut WriteHalf) {
            let messages = {
                let mut buf = self.pending.lock().await;
                std::mem::take(&mut *buf)
            };

            if messages.is_empty() {
                return;
            }

            tracing::debug!(
                target: "gpui_starter::websocket",
                count = messages.len(),
                "flushing pending messages"
            );

            for msg in &messages {
                if sink
                    .send(Message::Text(msg.clone()))
                    .await
                    .inspect_err(|e| {
                        tracing::warn!(
                            target: "gpui_starter::websocket",
                            error = %e,
                            "failed to send pending message"
                        );
                    })
                    .is_err()
                {
                    // Put remaining messages back into the buffer so they can
                    // be retried on the next connection attempt.
                    let idx = messages.iter().position(|m| m == msg).unwrap_or(0);
                    let remaining: Vec<String> = messages.into_iter().skip(idx).collect();
                    if !remaining.is_empty() {
                        let mut buf = self.pending.lock().await;
                        buf.splice(0..0, remaining);
                    }
                    return;
                }
            }
        }

        /// Connect to the WebSocket server with exponential-backoff retries.
        ///
        /// This is a self-contained async loop suitable for spawning via
        /// `cx.background_executor().spawn(...)` inside a `cx.spawn` block.
        ///
        /// After each successful connection the write half is stored in
        /// `self.inner` so that [`send`](Self::send) can route messages
        /// through it. Any messages buffered during a previous disconnection
        /// are flushed before the read loop begins.
        pub async fn connect_loop(&mut self) -> Result<(), WebSocketError> {
            let mut attempt: u8 = 0;

            loop {
                self.state = if attempt == 0 {
                    ConnectionState::Connecting
                } else {
                    ConnectionState::Reconnecting { attempt }
                };

                match connect_async(&self.url).await {
                    Ok((ws_stream, _response)) => {
                        tracing::info!(
                            target: "gpui_starter::websocket",
                            url = %self.url,
                            "connected"
                        );
                        self.state = ConnectionState::Connected;
                        attempt = 0;

                        // Split into write and read halves.
                        let (write, mut read) = ws_stream.split();

                        // Store the write half so `send()` can use it.
                        {
                            let mut guard = self.inner.lock().await;
                            *guard = Some(write);
                        }

                        // Flush any messages that were queued while we were
                        // disconnected.
                        {
                            let mut guard = self.inner.lock().await;
                            if let Some(ref mut sink) = *guard {
                                self.flush_pending(sink).await;
                            }
                        }

                        // Read messages until the stream closes or errors.
                        while let Some(msg) =
                            tokio_stream::StreamExt::next(&mut read).await
                        {
                            match msg {
                                Ok(Message::Text(text)) => {
                                    if let Some(ref handler) = self.on_message {
                                        handler(&text);
                                    }
                                }
                                Ok(Message::Close(frame)) => {
                                    tracing::info!(
                                        target: "gpui_starter::websocket",
                                        frame = ?frame,
                                        "server closed connection"
                                    );
                                    break;
                                }
                                Ok(_) => {} // binary, ping, pong — ignored for now
                                Err(e) => {
                                    tracing::warn!(
                                        target: "gpui_starter::websocket",
                                        error = %e,
                                        "read error"
                                    );
                                    break;
                                }
                            }
                        }

                        // Stream ended — fall through to reconnect.
                        self.state = ConnectionState::Disconnected;
                        {
                            let mut guard = self.inner.lock().await;
                            *guard = None;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "gpui_starter::websocket",
                            attempt,
                            error = %e,
                            "connection failed"
                        );
                    }
                }

                attempt += 1;
                if attempt > self.reconnect.max_retries {
                    tracing::error!(
                        target: "gpui_starter::websocket",
                        max_retries = self.reconnect.max_retries,
                        "exceeded max retries, giving up"
                    );
                    self.state = ConnectionState::Closed;
                    return Err(WebSocketError::Connection(format!(
                        "failed after {} retries",
                        self.reconnect.max_retries
                    )));
                }

                let delay = self.reconnect.delay_for_attempt(attempt - 1);
                tracing::info!(
                    target: "gpui_starter::websocket",
                    attempt,
                    delay_ms = delay.as_millis() as u64,
                    "waiting before reconnect"
                );
                tokio::time::sleep(delay).await;
            }
        }

        /// Send a text message over the active connection.
        ///
        /// If the socket is currently disconnected the message is buffered
        /// (up to [`MAX_PENDING_MESSAGES`]) and will be flushed automatically
        /// once the next connection is established.
        ///
        /// Returns [`WebSocketError::NotConnected`] only when the buffer is
        /// full and the message would be dropped.
        pub async fn send(&self, message: &str) -> Result<(), WebSocketError> {
            let mut guard = self.inner.lock().await;
            match guard.as_mut() {
                Some(sink) => sink
                    .send(Message::Text(message.into()))
                    .await
                    .map_err(WebSocketError::Send),
                None => {
                    // Not connected — buffer for later delivery.
                    drop(guard);
                    let mut buf = self.pending.lock().await;
                    if buf.len() >= MAX_PENDING_MESSAGES {
                        tracing::warn!(
                            target: "gpui_starter::websocket",
                            limit = MAX_PENDING_MESSAGES,
                            "pending buffer full, dropping message"
                        );
                        Err(WebSocketError::NotConnected)
                    } else {
                        buf.push(message.to_owned());
                        Ok(())
                    }
                }
            }
        }

        /// Gracefully close the WebSocket connection.
        ///
        /// Takes the write half out of the mutex (setting it to `None`) and
        /// sends a close frame. Also clears the pending buffer since no more
        /// messages will be delivered.
        pub async fn close(&mut self) -> Result<(), WebSocketError> {
            let sink = {
                let mut guard = self.inner.lock().await;
                guard.take()
            };

            if let Some(mut sink) = sink {
                sink.close().await.map_err(WebSocketError::Close)?;
            }

            // Clear any buffered messages — they will never be sent.
            {
                let mut buf = self.pending.lock().await;
                buf.clear();
            }

            self.state = ConnectionState::Closed;
            Ok(())
        }
    }
}

#[cfg(feature = "websocket")]
pub use live::WebSocketClient;

// ---------------------------------------------------------------------------
// Stub when the feature is disabled — keeps the module compilable.
// ---------------------------------------------------------------------------

#[cfg(not(feature = "websocket"))]
mod stub {
    use super::*;

    /// Placeholder WebSocket client (feature `websocket` is not enabled).
    ///
    /// Enable the feature and add `tokio-tungstenite` to `Cargo.toml`
    /// to get the real implementation.
    pub struct WebSocketClient {
        pub url: String,
        pub state: ConnectionState,
        pub reconnect: ReconnectPolicy,
    }

    impl WebSocketClient {
        pub fn new(url: String) -> Self {
            Self {
                url,
                state: ConnectionState::Disconnected,
                reconnect: ReconnectPolicy::default(),
            }
        }

        pub fn with_reconnect_policy(self, _policy: ReconnectPolicy) -> Self {
            self
        }

        /// No-op when the websocket feature is disabled.
        pub async fn connect_loop(&mut self) -> Result<(), WebSocketError> {
            Err(WebSocketError::FeatureDisabled)
        }

        /// No-op when the websocket feature is disabled.
        pub async fn send(&self, _message: &str) -> Result<(), WebSocketError> {
            Err(WebSocketError::FeatureDisabled)
        }

        /// No-op when the websocket feature is disabled.
        pub async fn close(&mut self) -> Result<(), WebSocketError> {
            Err(WebSocketError::FeatureDisabled)
        }
    }
}

#[cfg(not(feature = "websocket"))]
#[allow(unused_imports)]
pub use stub::WebSocketClient;

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
#[path = "websocket.test.rs"]
mod websocket_test;
