use gpui::prelude::*;
use gpui::*;

use gpui_component::{
    ActiveTheme as _, v_flex,
};

use gpui_query_v2::core::QueryStatus;

use super::PlaygroundUser;

// ---------------------------------------------------------------------------
// UI helpers
// ---------------------------------------------------------------------------

pub fn section_card(title: &str, description: &str, cx: &App) -> Div {
    div()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .overflow_hidden()
        .child(
            div()
                .px_4()
                .py_3()
                .bg(cx.theme().muted)
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    v_flex().gap_1()
                        .child(
                            div().text_base().font_weight(FontWeight::SEMIBOLD).child(title.to_string()),
                        )
                        .child(
                            div().text_xs().text_color(cx.theme().muted_foreground)
                                .child(description.to_string()),
                        ),
                ),
        )
}

pub fn mini_card(label: &str, cx: &App) -> Div {
    v_flex()
        .gap_2()
        .p_3()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .flex_1()
        .child(
            div().text_sm().font_weight(FontWeight::SEMIBOLD).child(label.to_string()),
        )
}

pub fn status_badge(status: QueryStatus, cx: &App) -> Div {
    let color = match status {
        QueryStatus::Idle => cx.theme().muted_foreground,
        QueryStatus::LoadingEmpty | QueryStatus::LoadingWithData => cx.theme().info,
        QueryStatus::Success => cx.theme().success,
        QueryStatus::Failure => cx.theme().danger,
        QueryStatus::Cancelled => cx.theme().warning,
    };
    div()
        .px_3()
        .py_1()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(color)
        .text_sm()
        .text_color(color)
        .child(status.label().to_string())
}

pub fn chip(label: &str, background: Hsla, cx: &App) -> Div {
    div()
        .px_3()
        .py_1()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(background)
        .text_sm()
        .child(label.to_string())
}

/// Finding 12: Render source preview with each entry on its own line instead
/// of joining with `\n` which may not render as visual line breaks in GPUI.
pub fn source_preview(data: &Option<Vec<PlaygroundUser>>) -> Div {
    match data {
        Some(users) => {
            let el = v_flex().gap_0p5();
            users.iter().fold(el, |el, u| {
                el.child(div().child(format!("{} ({}): {}", u.id, u.name, u.email)))
            })
        }
        None => v_flex().child(div().child("No data")),
    }
}

/// Finding 12: Render mapped preview as a single line (already was fine, but
/// now returns a Div for consistency with source_preview).
pub fn mapped_preview(data: &Option<Vec<String>>) -> Div {
    match data {
        Some(names) => v_flex().child(div().child(format!("[{}]", names.join(", ")))),
        None => v_flex().child(div().child("No data")),
    }
}
