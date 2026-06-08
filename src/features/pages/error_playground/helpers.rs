//! Helper functions for building error playground UI elements.

use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};

/// Build a test card with a colored left border (red = boundary, green = safe).
pub(crate) fn test_card(title: &str, description: &str, boundary: bool, cx: &App) -> Div {
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
pub(crate) fn action_row(_cx: &App) -> Div {
    h_flex()
        .gap_2()
        .flex_wrap()
        .items_center()
        .px_4()
        .py_3()
}

/// Inline result text chip.
pub(crate) fn result_inline(text: &str, cx: &App) -> Div {
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
