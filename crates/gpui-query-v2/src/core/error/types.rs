//! Core error types for query operations.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::sanitize::sanitize_message;

/// The kind of error that occurred during a query operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryErrorKind {
    /// The request was cancelled (cooperative cancellation).
    Cancelled,
    /// The server returned an error response.
    Response,
    /// A transport-level error occurred (network, timeout).
    Transport,
    /// An unknown error occurred.
    Unknown,
}

impl std::fmt::Display for QueryErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "cancelled"),
            Self::Response => write!(f, "response error"),
            Self::Transport => write!(f, "transport error"),
            Self::Unknown => write!(f, "unknown error"),
        }
    }
}

/// An error produced by a query or mutation operation.
///
/// Implements [`std::error::Error`] so it can be used with `?` propagation
/// and libraries like `anyhow`.
///
/// The internal message uses `Arc<str>` so cloning is cheap, making this
/// suitable for high-retry scenarios where the same error is stored in
/// multiple locations.
///
/// # Example
///
/// ```
/// use gpui_query_v2::QueryError;
///
/// let err = QueryError::response("not found");
/// assert_eq!(err.to_string(), "response error: not found");
/// ```
///
/// # Security
///
/// Error messages are included in debug/display output and may be serialized.
/// When constructing errors from server responses, sanitize the message first
/// or use [`QueryError::sanitized`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryError {
    pub(super) kind: QueryErrorKind,
    pub(super) message: Arc<str>,
}

impl QueryError {
    /// Create a new error with the given kind and message.
    pub fn new(kind: QueryErrorKind, message: impl Into<Arc<str>>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Create a cancellation error.
    pub fn cancelled(message: impl Into<Arc<str>>) -> Self {
        Self::new(QueryErrorKind::Cancelled, message)
    }

    /// Create a response error (server-side).
    pub fn response(message: impl Into<Arc<str>>) -> Self {
        Self::new(QueryErrorKind::Response, message)
    }

    /// Create a transport error (network, timeout).
    pub fn transport(message: impl Into<Arc<str>>) -> Self {
        Self::new(QueryErrorKind::Transport, message)
    }

    /// Create an unknown error.
    pub fn unknown(message: impl Into<Arc<str>>) -> Self {
        Self::new(QueryErrorKind::Unknown, message)
    }

    /// The kind of error.
    pub fn kind(&self) -> QueryErrorKind {
        self.kind
    }

    /// The error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Return a sanitized copy of this error with known sensitive patterns redacted.
    ///
    /// Redacts common patterns such as:
    /// - Database connection strings (`postgres://...`, `mysql://...`)
    /// - Bearer / token headers (`Bearer ...`, `token=...`)
    /// - File paths (`/home/...`, `/Users/...`, `/etc/...`)
    /// - Email-like strings
    /// - Long hex sequences (likely API keys)
    ///
    /// Also truncates the message to [`SANITIZE_MAX_LEN`](super::SANITIZE_MAX_LEN) bytes.
    pub fn sanitized(&self) -> Self {
        let redacted = sanitize_message(&self.message);
        Self {
            kind: self.kind,
            message: redacted.into(),
        }
    }
}
