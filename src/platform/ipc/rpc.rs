//! Encode/decode helpers and a pending-request registry for the typed
//! command/response layer (see [`super::command`]).
//!
//! The transport is gpui-starter's existing newline-JSON framing: each
//! [`ForwardedRequest`] / [`ForwardedResponse`] is serialized to a
//! single compact JSON line. This module adds no new codec — only
//! serde_json line adapters plus a synchronous `oneshot` map so a
//! caller can `await` (or block on) the matching reply.
//!
//! # Oneshot-per-request pattern
//!
//! Mirrors the request/response correlation in the reference launcher daemon
//! (`the reference launcher IPC server`, the `oneshot::channel` per call),
//! but rebinds it onto gpui-starter's std `tokio::sync::oneshot` and
//! the generic [`ForwardedCommand`] enum instead of the reference launcher's
//! `DaemonEvent`/`LauncherMode` types.

use std::collections::HashMap;

use serde::de::DeserializeOwned;
use tokio::sync::oneshot;

use super::command::{ForwardedRequest, ForwardedResponse};

const LOG: &str = "gpui_starter::ipc::rpc";

/// Encode a `Serialize`-able value as a single compact JSON line, with a
/// trailing `\n`. This is the exact shape the existing
/// [`super::IpcEndpoint::send`] / single-instance forwarder expect.
///
/// Returns an error on serialization failure rather than panicking; the
/// framing layer writes the returned bytes verbatim.
pub fn encode_line<T: serde::Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    Ok(line)
}

/// Decode a single JSON value from one frame (one line, already trimmed
/// of its trailing newline by the upstream reader). Empty/whitespace
/// input yields `Ok(None)` so callers can skip blank keepalive lines
/// without treating them as protocol errors.
pub fn decode_line<T: DeserializeOwned>(line: &str) -> Result<Option<T>, serde_json::Error> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(trimmed).map(Some)
}

/// Try to decode a line as a [`ForwardedRequest`].
///
/// Convenience wrapper around [`decode_line`] for the common server-side
/// path. Logs (does not panic) on malformed input.
pub fn decode_request(line: &str) -> Option<ForwardedRequest> {
    match decode_line::<ForwardedRequest>(line) {
        Ok(Some(req)) => Some(req),
        Ok(None) => None,
        Err(err) => {
            tracing::warn!(
                target: LOG,
                error = %err,
                line = line.trim(),
                "failed to decode ipc request line"
            );
            None
        }
    }
}

/// Try to decode a line as a [`ForwardedResponse`].
///
/// Convenience wrapper around [`decode_line`] for the common client-side
/// path. Returns `None` for blank lines and malformed JSON alike (the
/// latter is logged).
pub fn decode_response(line: &str) -> Option<ForwardedResponse> {
    match decode_line::<ForwardedResponse>(line) {
        Ok(Some(resp)) => Some(resp),
        Ok(None) => None,
        Err(err) => {
            tracing::warn!(
                target: LOG,
                error = %err,
                line = line.trim(),
                "failed to decode ipc response line"
            );
            None
        }
    }
}

/// A map of in-flight request ids to their `oneshot` reply senders.
///
/// The client side inserts a sender before writing the request line; the
/// reader loop resolves it with the matching [`ForwardedResponse`] (or
/// drops it on timeout/cancellation). This is the gpui-starter
/// equivalent of the reference launcher's per-call `oneshot::channel` correlation,
/// generalized to a registry so a single reader can multiplex many
/// concurrent requests.
#[derive(Debug, Default)]
pub struct PendingRequests {
    inner: HashMap<u64, oneshot::Sender<ForwardedResponse>>,
}

impl PendingRequests {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert the reply sender for `id`, returning any sender previously
    /// registered for that id (which is thereby cancelled).
    pub fn insert(
        &mut self,
        id: u64,
        sender: oneshot::Sender<ForwardedResponse>,
    ) -> Option<oneshot::Sender<ForwardedResponse>> {
        self.inner.insert(id, sender)
    }

    /// Remove and return the sender for `id`, if present. The caller is
    /// responsible for `.send()`ing the response.
    pub fn take(&mut self, id: u64) -> Option<oneshot::Sender<ForwardedResponse>> {
        self.inner.remove(&id)
    }

    /// Deliver a response to the waiting sender for `resp.id`, if any.
    ///
    /// Returns `true` if a waiter was present (the response was
    /// delivered or the receiver had already been dropped), `false` if
    /// no in-flight request matched the id. Never panics: a dropped
    /// receiver simply means the caller gave up (timeout/cancel).
    pub fn deliver(&mut self, resp: ForwardedResponse) -> bool {
        let id = resp.id;
        match self.inner.remove(&id) {
            Some(sender) => {
                // `send` failing means the waiter is gone; that's fine.
                let _ = sender.send(resp);
                true
            }
            None => {
                tracing::trace!(
                    target: LOG,
                    id,
                    ok = resp.ok,
                    "ipc response for unknown/late request; dropping"
                );
                false
            }
        }
    }

    /// Number of in-flight requests awaiting a reply.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the registry holds no pending requests.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Cancel every pending request by dropping its sender. Each waiter
    /// observes a `RecvError` and should treat it as cancellation. Safe
    /// to call on shutdown.
    pub fn clear(&mut self) {
        let count = self.inner.len();
        self.inner.clear();
        if count > 0 {
            tracing::debug!(target: LOG, cancelled = count, "cleared pending ipc requests");
        }
    }
}
