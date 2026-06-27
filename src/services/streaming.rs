//! Async-stream-to-GPUI plumbing.
//!
//! Bridges an arbitrary async [`Stream`] of `Result<String, _>` items into a
//! synchronous [`flume::Receiver`] that GPUI view code can poll from a
//! [`gpui::Task`].
//!
//! The stream is driven on gpui-starter's **shared** tokio runtime global
//! ([`crate::services::tokio_runtime::TokioRuntimeGlobal`]) — never on a
//! per-call `current_thread` runtime. GPUI's own executor is not a tokio
//! runtime, so any future that needs tokio (HTTP clients, `tokio::time`, …)
//! must be spawned through this shared runtime.
//!
//! # Completion + error semantics
//!
//! Tokens flow through the returned receiver as `String` values. The stream
//! completes when the receiver is closed (the producer drops its sender), which
//! happens as soon as the underlying stream yields `None` or an error. Errors
//! are mapped to `String`, logged under the `gpui_starter::streaming` target,
//! and surfaced as one final token-chunk followed by close — callers that need
//! to distinguish errors should prefer [`spawn_token_stream_with_errors`], which
//! yields a [`StreamUpdate`] enum instead.

use std::future::Future;

use flume::Receiver;
use futures_util::Stream;
use gpui::{App, AsyncApp, Context, Task, WeakEntity};

/// A single update from a streaming source, for callers that need to tell
/// tokens apart from terminal errors.
#[derive(Clone, Debug)]
pub enum StreamUpdate {
    /// A decoded token chunk to append to the view.
    Token(String),
    /// The stream failed terminally with this message.
    Error(String),
    /// The stream completed successfully.
    Done,
}

/// Drive an async stream of tokens into a [`flume::Receiver`] using gpui-starter's
/// shared tokio runtime global.
///
/// Returns the receiver plus a tokio [`JoinHandle`](tokio::task::JoinHandle)-like
/// task that the caller may keep alive (or drop to let the producer run to
/// completion in the background). Closing the receiver (dropping it) cancels the
/// producer: the send loop detects the closed channel and exits.
///
/// `stream` must be `Send + 'static` and yield `Result<String, E>` where `E:
/// std::fmt::Display`. Errors are logged and converted to an empty terminal
/// token; callers that need the error text should use
/// [`spawn_token_stream_with_errors`].
///
/// # Panics
/// Does not panic. If the tokio runtime global is missing the function logs a
/// warning and returns an already-closed receiver.
pub fn spawn_token_stream<S, E>(cx: &App, stream: S) -> (Task<()>, Receiver<String>)
where
    S: Stream<Item = Result<String, E>> + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let (tx, rx) = flume::unbounded::<String>();

    let runtime = cx
        .try_global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
        .map(|g| g.0.runtime.clone());

    let producer = cx.spawn(async move |_cx: &mut AsyncApp| {
        let Some(runtime) = runtime else {
            tracing::warn!(
                target: "gpui_starter::streaming",
                "TokioRuntimeGlobal not set; token stream cannot start"
            );
            // tx drops here, closing the channel.
            return;
        };

        // Drive the stream on the shared tokio runtime. The handle is detached
        // so the producer keeps running independently of this GPUI Task; the
        // GPUI Task just waits for it. Dropping the receiver cancels the loop.
        runtime
            .spawn(async move {
                use futures_util::StreamExt as _;
                let mut stream = std::pin::pin!(stream);
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(token) => {
                            if tx.send(token).is_err() {
                                // Receiver dropped — cancel the stream.
                                break;
                            }
                        }
                        Err(err) => {
                            tracing::warn!(
                                target: "gpui_starter::streaming",
                                error = %err,
                                "token stream produced an error; terminating"
                            );
                            // Surface the error as a final chunk so callers
                            // that render inline still see something, then stop.
                            let msg = err.to_string();
                            let _ = tx.send(msg);
                            break;
                        }
                    }
                }
                // tx drops at end of scope → receiver sees channel close.
            })
            .await
            .ok();
    });

    (producer, rx)
}

/// Like [`spawn_token_stream`] but yields [`StreamUpdate`] so callers can
/// distinguish terminal errors and clean completion from token chunks.
pub fn spawn_token_stream_with_errors<S, E>(
    cx: &App,
    stream: S,
) -> (Task<()>, Receiver<StreamUpdate>)
where
    S: Stream<Item = Result<String, E>> + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let (tx, rx) = flume::unbounded::<StreamUpdate>();

    let runtime = cx
        .try_global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
        .map(|g| g.0.runtime.clone());

    let producer = cx.spawn(async move |_cx: &mut AsyncApp| {
        let Some(runtime) = runtime else {
            tracing::warn!(
                target: "gpui_starter::streaming",
                "TokioRuntimeGlobal not set; token stream cannot start"
            );
            return;
        };

        runtime
            .spawn(async move {
                use futures_util::StreamExt as _;
                let mut stream = std::pin::pin!(stream);
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(token) => {
                            if tx.send(StreamUpdate::Token(token)).is_err() {
                                break;
                            }
                        }
                        Err(err) => {
                            tracing::warn!(
                                target: "gpui_starter::streaming",
                                error = %err,
                                "token stream produced an error; terminating"
                            );
                            let _ = tx.send(StreamUpdate::Error(err.to_string()));
                            return;
                        }
                    }
                }
                let _ = tx.send(StreamUpdate::Done);
            })
            .await
            .ok();
    });

    (producer, rx)
}

/// Spawn a polling loop that drains a `Receiver<String>` token channel and
/// forwards each token (plus completion/error detection via channel close) to a
/// caller-supplied callback that mutates the owning view.
///
/// `on_token` is invoked on the GPUI thread (inside `cx.update`) for every
/// token. The returned [`Task`] keeps the poll loop alive; dropping or replacing
/// it cancels polling — the canonical "cancel by replacing the Task" idiom.
///
/// The `weak` handle lets the loop short-circuit when the owning view is gone.
pub fn spawn_token_poller<T, F>(
    rx: Receiver<String>,
    weak: WeakEntity<T>,
    cx: &mut Context<T>,
    mut on_token: F,
) -> Task<()>
where
    T: 'static,
    F: FnMut(&mut T, &str) + Send + 'static,
{
    cx.spawn(async move |_this: WeakEntity<T>, cx: &mut AsyncApp| {
        // recv_async blocks until a token arrives or the producer closes.
        while let Ok(token) = rx.recv_async().await {
            let _ = cx.update(|cx: &mut App| {
                if let Some(this) = weak.upgrade() {
                    let _ = this.update(cx, |this, cx| {
                        on_token(this, &token);
                        cx.notify();
                    });
                }
            });
        }
        // Channel closed = stream finished. Notify the view so it can flip its
        // "streaming" flag off if it hasn't already.
        let _ = cx.update(|cx: &mut App| {
            if let Some(this) = weak.upgrade() {
                let _ = this.update(cx, |_, cx| {
                    cx.notify();
                });
            }
        });
    })
}

/// Spawn a polling loop for [`StreamUpdate`] receivers, with dedicated callbacks
/// for tokens, terminal errors, and clean completion. Each callback runs on the
/// GPUI thread.
#[allow(clippy::type_complexity)]
pub fn spawn_update_poller<T, Tok, Err, Done>(
    rx: Receiver<StreamUpdate>,
    weak: WeakEntity<T>,
    cx: &mut Context<T>,
    mut on_token: Tok,
    mut on_error: Err,
    mut on_done: Done,
) -> Task<()>
where
    T: 'static,
    Tok: FnMut(&mut T, &str) + Send + 'static,
    Err: FnMut(&mut T, &str) + Send + 'static,
    Done: FnMut(&mut T) + Send + 'static,
{
    cx.spawn(async move |_this: WeakEntity<T>, cx: &mut AsyncApp| {
        while let Ok(update) = rx.recv_async().await {
            let finished = matches!(update, StreamUpdate::Error(_) | StreamUpdate::Done);
            let _ = cx.update(|cx: &mut App| {
                if let Some(this) = weak.upgrade() {
                    let _ = this.update(cx, |this, cx| {
                        match update {
                            StreamUpdate::Token(t) => on_token(this, &t),
                            StreamUpdate::Error(e) => on_error(this, &e),
                            StreamUpdate::Done => on_done(this),
                        }
                        cx.notify();
                    });
                }
            });
            if finished {
                break;
            }
        }
    })
}

/// Convenience future wrapper for callers that already have a future (not a
/// [`Stream`]) that resolves to a single `String` result and want it funneled
/// through the same shared-runtime + channel plumbing.
pub fn spawn_future_stream<F, E>(cx: &App, future: F) -> (Task<()>, Receiver<StreamUpdate>)
where
    F: Future<Output = Result<String, E>> + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let (tx, rx) = flume::unbounded::<StreamUpdate>();

    let runtime = cx
        .try_global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
        .map(|g| g.0.runtime.clone());

    let producer = cx.spawn(async move |_cx: &mut AsyncApp| {
        let Some(runtime) = runtime else {
            tracing::warn!(
                target: "gpui_starter::streaming",
                "TokioRuntimeGlobal not set; future stream cannot start"
            );
            return;
        };

        runtime
            .spawn(async move {
                match future.await {
                    Ok(value) => {
                        let _ = tx.send(StreamUpdate::Token(value));
                    }
                    Err(err) => {
                        tracing::warn!(
                            target: "gpui_starter::streaming",
                            error = %err,
                            "future stream produced an error"
                        );
                        let _ = tx.send(StreamUpdate::Error(err.to_string()));
                    }
                }
                let _ = tx.send(StreamUpdate::Done);
            })
            .await
            .ok();
    });

    (producer, rx)
}
