//! Core types for the WebSocket client scaffold.
//!
//! These types are always available regardless of whether the `websocket`
//! feature is enabled.

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

/// Error type for WebSocket operations.
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
