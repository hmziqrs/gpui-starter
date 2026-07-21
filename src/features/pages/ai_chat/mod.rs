//! Optional AI chat page — an example of wiring
//! [`crate::services::streaming`] into a GPUI view.
//!
//! Compile-gated behind the `ai-chat` cargo feature (default OFF). The page is
//! fully self-contained: it defines its own local chat-turn model and a small
//! abstract trait ([`ChatStreamSource`]) that the caller implements to feed
//! tokens in — there is **no** dependency on any specific LLM crate. Any
//! `Stream<Item = Result<String, E>>` produced by the caller is driven on the
//! shared tokio runtime by [`crate::services::streaming::spawn_token_stream`].
//!
//! # Cancellation
//! Cancellation is "replace the [`Task`](gpui::Task)": sending a new prompt
//! overwrites the stored polling task with a ready no-op, dropping the previous
//! poller (and, transitively, dropping the receiver which cancels the producer).

pub mod view;

pub use view::AiResponseView;

use futures_util::Stream;
use gpui::{App, Task};

/// A participant in a chat exchange.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

/// One turn in a chat transcript.
#[derive(Clone, Debug)]
pub struct ChatTurn {
    pub role: Role,
    pub content: String,
}

impl ChatTurn {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// Abstract source of an assistant token stream.
///
/// The caller (the application wiring this page in) implements this to bridge
/// whatever backend it uses — an HTTP client, a local model, a test fixture —
/// into a `Stream<Item = Result<String, E>>`. The page itself never touches an
/// LLM crate, keeping the boilerplate backend-agnostic.
///
/// `messages` is the full transcript so far; the returned stream should produce
/// the assistant's reply as a sequence of string chunks. Errors are surfaced to
/// the view via [`AiResponseView::set_error`].
pub trait ChatStreamSource {
    /// Error produced by individual stream items; rendered via `Display`.
    type Error: std::fmt::Display + Send + 'static;
    /// The async stream type returned by [`Self::stream`].
    type Stream: Stream<Item = Result<String, Self::Error>> + Send + 'static;

    /// Begin streaming an assistant reply for the given transcript.
    ///
    /// Receives the gpui [`App`] so the implementor can pull globals (e.g. the
    /// shared tokio runtime / HTTP client) off `cx`.
    fn stream(&self, messages: &[ChatTurn], cx: &App) -> Self::Stream;
}

/// Handle returned when a streaming reply is kicked off. Dropping it (or
/// replacing the stored value) cancels the poll loop.
pub struct ChatStreamHandle {
    /// Polls the token channel and updates the view. Replace with
    /// `Task::ready(())` to cancel.
    pub poller: Task<()>,
}

impl ChatStreamHandle {
    /// Cancel the in-flight stream by replacing the poller with a no-op.
    pub fn cancel(&mut self) {
        self.poller = Task::ready(());
    }
}
