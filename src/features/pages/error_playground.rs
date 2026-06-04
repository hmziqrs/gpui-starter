//! Error Boundary Playground — interactive test page for the error boundary.
//!
//! Each card exercises a different failure mode. Cards marked "boundary" (red
//! left-border) will panic during render and trigger the error boundary.
//! Cards marked "safe" (green left-border) handle errors gracefully inline.

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

// ---------------------------------------------------------------------------
// Page struct
// ---------------------------------------------------------------------------

pub struct ErrorPlaygroundPage {
    // Render-path trigger flags (checked at the top of render).
    trigger_render_panic: bool,
    trigger_div_zero: bool,
    trigger_oob: bool,
    // Inline results for safe tests.
    http_result: Option<String>,
    fs_result: Option<String>,
    async_result: Option<String>,
    background_panic_result: Option<String>,
}

impl ErrorPlaygroundPage {
    pub fn new() -> Self {
        Self {
            trigger_render_panic: false,
            trigger_div_zero: false,
            trigger_oob: false,
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
        // IMPORTANT: Check trigger flags FIRST, before any other rendering.
        // These panics happen during render so the error boundary can catch them.
        if self.trigger_render_panic {
            panic!("error playground: intentional render panic");
        }
        if self.trigger_div_zero {
            let _ = 1 / (1 - self.trigger_div_zero as i32);
        }
        if self.trigger_oob {
            let _ = vec![0u8][100];
        }

        let theme = cx.theme();
        let radius_lg = theme.radius_lg;
        let border = theme.border;
        let muted = theme.muted;
        let muted_foreground = theme.muted_foreground;
        let _background = theme.background;
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
                                    "Test different failure modes. Red-bordered cards trigger the \
                                     error boundary (render-path panics). Green-bordered cards \
                                     handle errors gracefully inline.",
                                ),
                        ),
                    ),
            )
            // -- 1. Render Panic --
            .child(self.render_render_panic(cx))
            // -- 2. Division by Zero --
            .child(self.render_div_zero(cx))
            // -- 3. Index Out of Bounds --
            .child(self.render_oob(cx))
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
    fn render_render_panic(&self, cx: &mut Context<Self>) -> Div {
        test_card(
            "Render Panic",
            "Panics directly in the render method. Should trigger the error boundary \
             and show the fallback page.",
            true, // boundary test
            cx,
        )
        .child(
            action_row(cx).child(
                Button::new("ep-render-panic")
                    .primary()
                    .label("Trigger Render Panic")
                    .on_click(cx.listener(|this, _, _, _cx| {
                        this.trigger_render_panic = true;
                    })),
            ),
        )
    }

    fn render_div_zero(&self, cx: &mut Context<Self>) -> Div {
        test_card(
            "Division by Zero",
            "Causes a division-by-zero panic during render. Should trigger the error boundary.",
            true,
            cx,
        )
        .child(
            action_row(cx).child(
                Button::new("ep-div-zero")
                    .primary()
                    .label("Trigger Division by Zero")
                    .on_click(cx.listener(|this, _, _, _cx| {
                        this.trigger_div_zero = true;
                    })),
            ),
        )
    }

    fn render_oob(&self, cx: &mut Context<Self>) -> Div {
        test_card(
            "Index Out of Bounds",
            "Accesses an out-of-bounds index during render. Should trigger the error boundary.",
            true,
            cx,
        )
        .child(
            action_row(cx).child(
                Button::new("ep-oob")
                    .primary()
                    .label("Trigger Index Out of Bounds")
                    .on_click(cx.listener(|this, _, _, _cx| {
                        this.trigger_oob = true;
                    })),
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
