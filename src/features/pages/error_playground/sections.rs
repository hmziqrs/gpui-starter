//! Section renderers for each error playground test card.

use gpui::{prelude::*, *};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    v_flex,
};

use super::super::render_error::TriggerRenderError;
use super::ErrorPlaygroundPage;
use super::helpers::{action_row, result_inline, test_card};

/// Parameter bundle for the unified HTTP/timeout error block renderer.
///
/// Every field is `Copy`, so a single value can be captured into the click
/// closure and reused across invocations without cloning.
#[derive(Clone, Copy)]
struct ErrorBlockCtx {
    title: &'static str,
    description: &'static str,
    button_key: &'static str,
    button_label: &'static str,
    initial_msg: &'static str,
    url: &'static str,
    timeout: std::time::Duration,
    error_prefix: &'static str,
}

impl ErrorPlaygroundPage {
    pub(super) fn render_boundary_trigger(
        &self,
        title: &str,
        description: &str,
        button_label: &str,
        error_message: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let error_msg = error_message.to_string();
        test_card(title, description, true, cx).child(
            action_row(cx).child(
                Button::new(SharedString::from(format!("ep-{}", title)))
                    .primary()
                    .label(button_label.to_string())
                    .on_click(move |_, _, cx| {
                        // Activate the error boundary by dispatching the action.
                        // AppRoot listens for this and swaps in RenderErrorPage.
                        cx.dispatch_action(&TriggerRenderError {
                            message: error_msg.clone(),
                        });
                    }),
            ),
        )
    }

    pub(super) fn render_background_panic(&mut self, cx: &mut Context<Self>) -> Div {
        let result_text = self.background_panic_result.clone();
        let card = test_card(
            "Background Task Panic",
            "Spawns an async task on the tokio runtime that panics. Should NOT trigger the \
             error boundary (only render-path panics are caught by the thread-local guard).",
            false,
            cx,
        );

        card.child(
            v_flex().gap_2().px_4().pb_3().child(
                action_row(cx)
                    .child(
                        Button::new("ep-bg-panic")
                            .primary()
                            .label("Spawn Background Panic")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.background_panic_result =
                                    Some("Panic spawned — check logs".to_string());
                                cx.notify();

                                let rt = cx
                                    .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
                                    .0
                                    .runtime
                                    .clone();
                                let _handle = rt.spawn(async move {
                                    panic!("error playground: background panic");
                                });
                            })),
                    )
                    .when_some(result_text, |el, text| el.child(result_inline(&text, cx))),
            ),
        )
    }

    pub(super) fn render_http_error(&mut self, cx: &mut Context<Self>) -> Div {
        let result_text = self.http_result.clone();
        Self::render_error_block(
            ErrorBlockCtx {
                title: "HTTP Error",
                description: "Sends an HTTP request to an invalid URL (127.0.0.1:1). Should NOT \
                    trigger the error boundary — the error is shown inline.",
                button_key: "ep-http-error",
                button_label: "Send HTTP Request",
                initial_msg: "Requesting...",
                url: "http://127.0.0.1:1/fail",
                timeout: std::time::Duration::from_secs(5),
                error_prefix: "HTTP error",
            },
            |this, v| this.http_result = v,
            result_text,
            cx,
        )
    }

    pub(super) fn render_fs_error(&mut self, cx: &mut Context<Self>) -> Div {
        let result_text = self.fs_result.clone();
        let card = test_card(
            "Filesystem Error",
            "Tries to read a file that does not exist. Should NOT trigger the error \
             boundary — the error is shown inline.",
            false,
            cx,
        );

        card.child(
            v_flex().gap_2().px_4().pb_3().child(
                action_row(cx)
                    .child(
                        Button::new("ep-fs-error")
                            .primary()
                            .label("Read Missing File")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let path = "/tmp/gpui-error-playground-nonexistent.txt";
                                match std::fs::read_to_string(path) {
                                    Ok(_contents) => {
                                        this.fs_result =
                                            Some(format!("Unexpected success reading {}", path));
                                    }
                                    Err(e) => {
                                        this.fs_result = Some(format!("FS error: {e}"));
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .when_some(result_text, |el, text| el.child(result_inline(&text, cx))),
            ),
        )
    }

    pub(super) fn render_async_timeout(&mut self, cx: &mut Context<Self>) -> Div {
        let result_text = self.async_result.clone();
        Self::render_error_block(
            ErrorBlockCtx {
                title: "Async Timeout",
                description: "Sends an HTTP request with a very short timeout (1ms) to a slow \
                    endpoint. Should NOT trigger the error boundary — the timeout error is \
                    shown inline.",
                button_key: "ep-async-timeout",
                button_label: "Send Timeout Request",
                initial_msg: "Requesting (1ms timeout)...",
                url: "http://httpbin.org/delay/5",
                timeout: std::time::Duration::from_millis(1),
                error_prefix: "Timeout error",
            },
            |this, v| this.async_result = v,
            result_text,
            cx,
        )
    }

    /// Unified renderer behind `render_http_error` and `render_async_timeout`.
    ///
    /// The two cards are structurally identical; they differ only in URL,
    /// timeout, copy, and which result field receives updates. The `set_result`
    /// callback isolates that last difference so the shared spawn/match logic
    /// lives in exactly one place. The `TokioRuntimeGlobal` is read once per
    /// click and both handles are pulled from that single borrow.
    fn render_error_block(
        ctx: ErrorBlockCtx,
        set_result: impl Fn(&mut Self, Option<String>) + Copy + Send + 'static,
        result_text: Option<String>,
        cx: &mut Context<Self>,
    ) -> Div {
        let card = test_card(ctx.title, ctx.description, false, cx);

        card.child(
            v_flex().gap_2().px_4().pb_3().child(
                action_row(cx)
                    .child(
                        Button::new(ctx.button_key)
                            .primary()
                            .label(ctx.button_label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                set_result(this, Some(ctx.initial_msg.to_string()));
                                cx.notify();

                                // Single borrow of the tokio runtime global for both handles
                                // (previously this read the global twice per click).
                                let tokio_rt = cx
                                    .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>();
                                let rt = tokio_rt.0.runtime.clone();
                                let client = tokio_rt.0.http_client.clone();

                                cx.spawn(async move |this, cx| {
                                    let result = rt
                                        .spawn(async move {
                                            client.get(ctx.url).timeout(ctx.timeout).send().await
                                        })
                                        .await;

                                    let msg = match result {
                                        Ok(Ok(resp)) => {
                                            format!("Unexpected success: status {}", resp.status())
                                        }
                                        Ok(Err(e)) => format!("{}: {e}", ctx.error_prefix),
                                        Err(e) => format!("Task panicked: {e}"),
                                    };

                                    this.update(cx, |this, cx| {
                                        set_result(this, Some(msg));
                                        cx.notify();
                                    })
                                    .ok();
                                })
                                .detach();
                            })),
                    )
                    .when_some(result_text, |el, text| el.child(result_inline(&text, cx))),
            ),
        )
    }

    pub(super) fn render_clear_results(&self, cx: &mut Context<Self>) -> Div {
        let card = test_card(
            "Clear Results",
            "Resets all inline error/success messages.",
            false,
            cx,
        );

        card.child(
            action_row(cx).child(
                Button::new("ep-clear")
                    .outline()
                    .label("Clear All Results")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.http_result = None;
                        this.fs_result = None;
                        this.async_result = None;
                        this.background_panic_result = None;
                        cx.notify();
                    })),
            ),
        )
    }
}
