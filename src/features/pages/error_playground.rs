//! Error Boundary Playground — interactive test page for the error boundary.
//!
//! ## Architecture note
//!
//! GPUI render panics are process-fatal: they propagate through an `extern "C"`
//! Metal rendering callback where Rust's unwinder cannot safely unwind,
//! producing `"fatal runtime error: failed to initiate panic, error 3"`.
//!
//! Because of this, the "boundary" test cards below do NOT actually panic
//! during render. Instead they dispatch a `TriggerRenderError` action that
//! `AppRoot` intercepts to activate the error boundary UI directly. This tests
//! the full recovery flow (fallback page → reload → retry) without crashing.
//!
//! The "safe" cards (green border) exercise real error paths (HTTP, FS, async)
//! that are handled gracefully inline.

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

use super::render_error::TriggerRenderError;

// ---------------------------------------------------------------------------
// Page struct
// ---------------------------------------------------------------------------

pub struct ErrorPlaygroundPage {
    // Inline results for safe tests.
    http_result: Option<String>,
    fs_result: Option<String>,
    async_result: Option<String>,
    background_panic_result: Option<String>,
}

impl ErrorPlaygroundPage {
    pub fn new() -> Self {
        Self {
            http_result: None,
            fs_result: None,
            async_result: None,
            background_panic_result: None,
        }
    }
}

impl Default for ErrorPlaygroundPage {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

impl Render for ErrorPlaygroundPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let radius_lg = theme.radius_lg;
        let border = theme.border;
        let muted = theme.muted;
        let muted_foreground = theme.muted_foreground;
        let _ = theme;

        v_flex()
            .id("error-playground-page")
            .min_h_full()
            .p_6()
            .gap_5()
            .overflow_y_scroll()
            // -- Header --
            .child(
                div()
                    .p_5()
                    .rounded(radius_lg)
                    .border_1()
                    .border_color(border)
                    .bg(muted)
                    .child(
                        v_flex().gap_3().child(
                            div().text_2xl().font_weight(FontWeight::BOLD)
                                .child("Error Boundary Playground"),
                        ).child(
                            div()
                                .max_w(px(800.))
                                .text_sm()
                                .text_color(muted_foreground)
                                .child(
                                    "Test different failure modes. Red-bordered cards activate the \
                                     error boundary via action dispatch (simulating a render panic \
                                     without crashing the process). Green-bordered cards handle \
                                     errors gracefully inline. See the architecture note at the top \
                                     of this file for why render panics cannot be caught at runtime.",
                                ),
                        ),
                    ),
            )
            // -- 1. Simulated Render Error --
            .child(self.render_boundary_trigger(
                "Simulated Render Error",
                "Dispatches TriggerRenderError to activate the error boundary directly. \
                 The fallback page appears with a summary and Reload button. \
                 Note: a real render panic is process-fatal in GPUI (Metal extern \"C\" callback), \
                 so this simulates the recovery flow without crashing.",
                "Trigger Render Error",
                "error playground: simulated render panic",
                cx,
            ))
            // -- 2. Simulated Division by Zero --
            .child(self.render_boundary_trigger(
                "Simulated Division by Zero",
                "Activates the error boundary as if a division-by-zero occurred during render. \
                 Same mechanism as above — the boundary UI is tested end-to-end.",
                "Trigger Div Zero Error",
                "error playground: simulated division by zero",
                cx,
            ))
            // -- 3. Simulated Index Out of Bounds --
            .child(self.render_boundary_trigger(
                "Simulated Index Out of Bounds",
                "Activates the error boundary as if an out-of-bounds access occurred during render.",
                "Trigger OOB Error",
                "error playground: simulated index out of bounds",
                cx,
            ))
            // -- 4. Background Task Panic --
            .child(self.render_background_panic(cx))
            // -- 5. HTTP Error --
            .child(self.render_http_error(cx))
            // -- 6. Filesystem Error --
            .child(self.render_fs_error(cx))
            // -- 7. Async Timeout --
            .child(self.render_async_timeout(cx))
            // -- 8. Clear Results --
            .child(self.render_clear_results(cx))
    }
}

// ---------------------------------------------------------------------------
// Section renderers
// ---------------------------------------------------------------------------

impl ErrorPlaygroundPage {
    fn render_boundary_trigger(
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

    fn render_background_panic(&mut self, cx: &mut Context<Self>) -> Div {
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
                    .when_some(result_text, |el, text| {
                        el.child(result_inline(&text, cx))
                    }),
            ),
        )
    }

    fn render_http_error(&mut self, cx: &mut Context<Self>) -> Div {
        let result_text = self.http_result.clone();
        let card = test_card(
            "HTTP Error",
            "Sends an HTTP request to an invalid URL (127.0.0.1:1). Should NOT trigger \
             the error boundary — the error is shown inline.",
            false,
            cx,
        );

        card.child(
            v_flex().gap_2().px_4().pb_3().child(
                action_row(cx)
                    .child(
                        Button::new("ep-http-error")
                            .primary()
                            .label("Send HTTP Request")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.http_result = Some("Requesting...".to_string());
                                cx.notify();

                                let rt = cx
                                    .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
                                    .0
                                    .runtime
                                    .clone();
                                let client = cx
                                    .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
                                    .0
                                    .http_client
                                    .clone();

                                cx.spawn(async move |this, cx| {
                                    let result = rt
                                        .spawn(async move {
                                            client
                                                .get("http://127.0.0.1:1/fail")
                                                .timeout(std::time::Duration::from_secs(5))
                                                .send()
                                                .await
                                        })
                                        .await;

                                    let msg = match result {
                                        Ok(Ok(resp)) => {
                                            format!("Unexpected success: status {}", resp.status())
                                        }
                                        Ok(Err(e)) => format!("HTTP error: {e}"),
                                        Err(e) => format!("Task panicked: {e}"),
                                    };

                                    this.update(cx, |this, cx| {
                                        this.http_result = Some(msg);
                                        cx.notify();
                                    })
                                    .ok();
                                })
                                .detach();
                            })),
                    )
                    .when_some(result_text, |el, text| {
                        el.child(result_inline(&text, cx))
                    }),
            ),
        )
    }

    fn render_fs_error(&mut self, cx: &mut Context<Self>) -> Div {
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
                    .when_some(result_text, |el, text| {
                        el.child(result_inline(&text, cx))
                    }),
            ),
        )
    }

    fn render_async_timeout(&mut self, cx: &mut Context<Self>) -> Div {
        let result_text = self.async_result.clone();
        let card = test_card(
            "Async Timeout",
            "Sends an HTTP request with a very short timeout (1ms) to a slow endpoint. \
             Should NOT trigger the error boundary — the timeout error is shown inline.",
            false,
            cx,
        );

        card.child(
            v_flex().gap_2().px_4().pb_3().child(
                action_row(cx)
                    .child(
                        Button::new("ep-async-timeout")
                            .primary()
                            .label("Send Timeout Request")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.async_result = Some("Requesting (1ms timeout)...".to_string());
                                cx.notify();

                                let rt = cx
                                    .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
                                    .0
                                    .runtime
                                    .clone();
                                let client = cx
                                    .global::<crate::services::tokio_runtime::TokioRuntimeGlobal>()
                                    .0
                                    .http_client
                                    .clone();

                                cx.spawn(async move |this, cx| {
                                    let result = rt
                                        .spawn(async move {
                                            client
                                                .get("http://httpbin.org/delay/5")
                                                .timeout(std::time::Duration::from_millis(1))
                                                .send()
                                                .await
                                        })
                                        .await;

                                    let msg = match result {
                                        Ok(Ok(resp)) => {
                                            format!("Unexpected success: status {}", resp.status())
                                        }
                                        Ok(Err(e)) => format!("Timeout error: {e}"),
                                        Err(e) => format!("Task panicked: {e}"),
                                    };

                                    this.update(cx, |this, cx| {
                                        this.async_result = Some(msg);
                                        cx.notify();
                                    })
                                    .ok();
                                })
                                .detach();
                            })),
                    )
                    .when_some(result_text, |el, text| {
                        el.child(result_inline(&text, cx))
                    }),
            ),
        )
    }

    fn render_clear_results(&self, cx: &mut Context<Self>) -> Div {
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a test card with a colored left border (red = boundary, green = safe).
fn test_card(title: &str, description: &str, boundary: bool, cx: &App) -> Div {
    let theme = cx.theme();
    let accent = if boundary {
        theme.danger
    } else {
        theme.success
    };

    // Outer wrapper: horizontal flex with colored stripe + card body.
    h_flex()
        .rounded(theme.radius_lg)
        .overflow_hidden()
        // Colored accent stripe on the left
        .child(
            div()
                .h_full()
                .w(px(4.))
                .bg(accent)
                .flex_shrink_0(),
        )
        // Card body
        .child(
            div()
                .flex_1()
                .border_1()
                .border_color(theme.border)
                .overflow_hidden()
                .child(
                    div()
                        .px_4()
                        .py_3()
                        .bg(theme.muted)
                        .border_b_1()
                        .border_color(theme.border)
                        .child(
                            v_flex().gap_1()
                                .child(
                                    div().text_base().font_weight(FontWeight::SEMIBOLD)
                                        .child(title.to_string()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(description.to_string()),
                                ),
                        ),
                ),
        )
}

/// Row that holds the trigger button(s) and any inline result.
fn action_row(_cx: &App) -> Div {
    h_flex()
        .gap_2()
        .flex_wrap()
        .items_center()
        .px_4()
        .py_3()
}

/// Inline result text chip.
fn result_inline(text: &str, cx: &App) -> Div {
    let theme = cx.theme();
    let is_error = text.to_lowercase().contains("error")
        || text.to_lowercase().contains("panic")
        || text.to_lowercase().contains("timeout");

    let color = if is_error {
        theme.danger
    } else {
        theme.muted_foreground
    };

    div()
        .text_xs()
        .text_color(color)
        .px_2()
        .py_1()
        .rounded(theme.radius)
        .bg(theme.background)
        .border_1()
        .border_color(color)
        .child(text.to_string())
}
