//! [`AiResponseView`] — renders a streaming chat transcript with user bubbles,
//! assistant markdown, a "Thinking…" placeholder, and a U+258C streaming cursor.

use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme as _, text::markdown, v_flex};

use super::{ChatStreamSource, ChatTurn, Role};

/// The full-width U+258C (LEFT HALF BLOCK) cursor appended to a streaming
/// assistant message.
const STREAMING_CURSOR: char = '\u{258C}';

/// View holding a chat transcript and driving a streaming assistant reply.
///
/// This owns the chat-turn model ([`Vec<ChatTurn>`]) plus streaming/error state.
/// It is deliberately backend-agnostic: tokens arrive via [`Self::append_token`]
/// and the caller is free to feed them from
/// [`crate::services::streaming::spawn_token_stream`] or any other source.
#[derive(Clone)]
pub struct AiResponseView {
    turns: Vec<ChatTurn>,
    is_streaming: bool,
    error: Option<String>,
}

impl AiResponseView {
    /// Create a view seeded with a user prompt and an empty (streaming)
    /// assistant reply.
    pub fn new(prompt: impl Into<String>) -> Self {
        let prompt = prompt.into();
        tracing::debug!(
            target: "gpui_starter::features::pages::ai_chat",
            prompt_len = prompt.len(),
            "AiResponseView created"
        );
        Self {
            turns: vec![ChatTurn::user(prompt), ChatTurn::assistant(String::new())],
            is_streaming: true,
            error: None,
        }
    }

    /// Create an empty (non-streaming) view.
    pub fn empty() -> Self {
        Self {
            turns: Vec::new(),
            is_streaming: false,
            error: None,
        }
    }

    /// Append a decoded token to the latest assistant turn. No-op if there is
    /// no assistant turn yet.
    pub fn append_token(&mut self, token: &str) {
        if let Some(last) = self.turns.last_mut() {
            if last.role == Role::Assistant {
                last.content.push_str(token);
            }
        }
    }

    /// Mark the current assistant reply as complete.
    pub fn finish_streaming(&mut self) {
        if self.is_streaming {
            tracing::debug!(
                target: "gpui_starter::features::pages::ai_chat",
                "assistant stream finished"
            );
        }
        self.is_streaming = false;
    }

    /// Record a terminal error and stop streaming.
    pub fn set_error(&mut self, error: impl Into<String>) {
        let error = error.into();
        tracing::warn!(
            target: "gpui_starter::features::pages::ai_chat",
            error = %error,
            "assistant stream errored"
        );
        self.error = Some(error);
        self.is_streaming = false;
    }

    /// Begin a new exchange: append a user message and a fresh empty assistant
    /// turn, clear any prior error, and mark streaming.
    pub fn add_user_message(&mut self, message: impl Into<String>) {
        self.error = None;
        self.turns.push(ChatTurn::user(message));
        self.turns.push(ChatTurn::assistant(String::new()));
        self.is_streaming = true;
    }

    /// The transcript so far (used by a [`ChatStreamSource`] implementor to build
    /// the next request).
    pub fn turns(&self) -> &[ChatTurn] {
        &self.turns
    }

    /// Whether an assistant reply is currently streaming.
    pub fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    /// Whether the view is in an error state.
    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }
}

impl AiResponseView {
    /// Render the full transcript (or the error banner) as a GPUI element tree.
    ///
    /// User turns render as right-aligned bubbles; assistant turns render as
    /// markdown (via `gpui_component::text::markdown`) — except while the
    /// assistant has produced no text yet, in which case a "Thinking…"
    /// placeholder is shown. A streaming assistant turn gets a trailing
    /// `STREAMING_CURSOR`.
    pub fn render(&self, _window: &mut Window, cx: &mut App) -> Div {
        let theme = cx.theme();
        let accent = theme.accent;
        let muted = theme.muted_foreground;
        let foreground = theme.foreground;
        let border = theme.border;
        let danger = theme.danger;

        let mut container = div().w_full().h_full().flex().flex_col().gap_3().p_0();

        if let Some(error) = &self.error {
            container = container.child(
                div()
                    .id("ai-chat-error")
                    .flex_1()
                    .w_full()
                    .p_4()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .bg(danger.opacity(0.12))
                            .border_1()
                            .border_color(danger.opacity(0.4))
                            .rounded_md()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(danger)
                                    .child(SharedString::from("Error")),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(foreground)
                                    .child(SharedString::from(error.clone())),
                            ),
                    ),
            );
            return container;
        }

        let mut messages = v_flex().w_full().p_4().gap_3();

        let last_index = self.turns.len().saturating_sub(1);
        for (i, turn) in self.turns.iter().enumerate() {
            let is_last = i == last_index;
            let streaming_here = is_last && self.is_streaming && turn.role == Role::Assistant;
            match turn.role {
                Role::User => {
                    messages = messages.child(render_user_bubble(
                        i,
                        &turn.content,
                        accent,
                        foreground,
                        border,
                    ));
                }
                Role::Assistant => {
                    messages = messages.child(render_assistant_message(
                        i,
                        &turn.content,
                        streaming_here,
                        muted,
                    ));
                }
            }
        }

        container.child(
            div()
                .id("ai-chat-scroll")
                .flex_1()
                .w_full()
                .overflow_y_scroll()
                .child(messages),
        )
    }
}

/// Right-aligned user bubble.
fn render_user_bubble(
    index: usize,
    content: &str,
    accent: Hsla,
    text_color: Hsla,
    border: Hsla,
) -> impl IntoElement {
    div()
        .id(ElementId::Name(format!("ai-chat-user-{}", index).into()))
        .w_full()
        .flex()
        .justify_end()
        .child(
            div()
                .max_w(px(520.))
                .px_3()
                .py_2()
                .bg(accent.opacity(0.16))
                .border_1()
                .border_color(border)
                .rounded_lg()
                .text_sm()
                .text_color(text_color)
                .whitespace_normal()
                .child(SharedString::from(content.to_string())),
        )
}

/// Assistant turn: "Thinking…" placeholder, markdown body, or markdown + cursor.
fn render_assistant_message(
    index: usize,
    content: &str,
    streaming: bool,
    muted: Hsla,
) -> impl IntoElement {
    let wrapper = div().id(ElementId::Name(
        format!("ai-chat-assistant-{}", index).into(),
    ));

    if content.is_empty() && streaming {
        // Thinking placeholder before the first token arrives.
        wrapper.child(
            div()
                .text_sm()
                .italic()
                .text_color(muted)
                .child(SharedString::from("Thinking…")),
        )
    } else {
        let display = if streaming {
            format!("{}{}", content, STREAMING_CURSOR)
        } else {
            content.to_string()
        };
        // gpui-component's markdown() returns a TextView implementing IntoElement.
        // If markdown rendering is unavailable in a given build, fall back to
        // plain text by treating the same string as a SharedString.
        wrapper.child(markdown(SharedString::from(display)))
    }
}

/// Kick off a streaming assistant reply for the current transcript and return a
/// poller [`Task`] that keeps the view updated. Driving the stream happens on
/// the shared tokio runtime via [`crate::services::streaming::spawn_token_stream`].
///
/// `E` is the entity that owns the [`AiResponseView`] (e.g. the page). `apply`
/// bridges a token into a mutation on that entity — typically by updating the
/// embedded `AiResponseView` with [`AiResponseView::append_token`]. The caller
/// is responsible for calling [`AiResponseView::finish_streaming`] /
/// [`AiResponseView::set_error`] when the stream ends (the receiver close is
/// observable as the poller's final `cx.notify()`; see
/// [`crate::services::streaming::spawn_token_poller`]).
///
/// The returned [`Task`] is the canonical cancellation handle: replace it with
/// `Task::ready(())` to cancel. The producer is detached so it runs to
/// completion independent of the poller; cancelling the poller drops the
/// receiver, which the producer detects and exits.
pub fn start_stream<E, S, F>(
    view: &AiResponseView,
    weak: WeakEntity<E>,
    source: &S,
    cx: &mut Context<E>,
    apply: F,
) -> Option<Task<()>>
where
    E: 'static,
    S: ChatStreamSource,
    F: FnMut(&mut E, &str) + Send + 'static,
{
    // Snapshot the transcript so the source can build its request.
    let messages: Vec<ChatTurn> = view.turns().to_vec();
    let stream = source.stream(&messages, cx);

    let (producer, rx) = crate::services::streaming::spawn_token_stream(cx, stream);

    let poller = crate::services::streaming::spawn_token_poller(rx, weak, cx, apply);

    // Keep the producer alive for the lifetime of the poller by detaching it.
    producer.detach();

    Some(poller)
}
