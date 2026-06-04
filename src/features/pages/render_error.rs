use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _,
    button::Button,
    v_flex,
};

// ---------------------------------------------------------------------------
// Action: reload the page that triggered the render panic
// ---------------------------------------------------------------------------

/// Clear the error boundary and retry rendering the active page.
#[derive(Action, Clone, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = app, no_json)]
pub struct ReloadCurrentPage;

// ---------------------------------------------------------------------------
// RenderErrorPage
// ---------------------------------------------------------------------------

/// Fallback view displayed by the error boundary when a page's render method
/// panics. Shows a human-readable summary and a "Reload" button that clears
/// the error state and retries the original page.
pub struct RenderErrorPage {
    summary: String,
}

impl RenderErrorPage {
    pub fn new(summary: String) -> Self {
        Self { summary }
    }
}

impl Render for RenderErrorPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let summary = self.summary.clone();

        v_flex()
            .min_h_full()
            .items_center()
            .justify_center()
            .gap_4()
            .p_8()
            .child(
                v_flex()
                    .items_center()
                    .gap_3()
                    .max_w(px(480.))
                    // Error title
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(cx.theme().danger)
                            .child("Render Error"),
                    )
                    // Summary text
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(SharedString::from(summary)),
                    )
                    // Reload button
                    .child(
                        Button::new("reload-current-page")
                            .label("Reload Page")
                            .on_click(|_, _, cx| {
                                cx.dispatch_action(&ReloadCurrentPage);
                            }),
                    ),
            )
    }
}
