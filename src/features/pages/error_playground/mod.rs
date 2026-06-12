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

mod helpers;
mod sections;

use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme as _, v_flex};

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

#[cfg(test)]
#[path = "../error_playground.test.rs"]
mod error_playground_test;
