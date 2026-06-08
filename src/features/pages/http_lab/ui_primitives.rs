use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme as _, v_flex};

use gpui_query::QueryStatus;

pub fn panel(title: &str, cx: &App) -> Div {
    v_flex()
        .gap_3()
        .p_4()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .child(section_title(title))
}

pub fn section_title(title: &str) -> Div {
    div()
        .text_lg()
        .font_weight(FontWeight::BOLD)
        .child(title.to_string())
}

pub fn kv(label: &str, value: &str, cx: &App) -> Div {
    div()
        .flex()
        .gap_2()
        .text_sm()
        .child(
            div()
                .min_w(px(150.))
                .font_weight(FontWeight::BOLD)
                .child(format!("{label}:")),
        )
        .child(
            div()
                .flex_1()
                .text_color(cx.theme().muted_foreground)
                .child(value.to_string()),
        )
}

pub fn preview_block(label: &str, value: &str, cx: &App) -> Div {
    v_flex()
        .gap_1()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .child(label.to_string()),
        )
        .child(
            div()
                .p_3()
                .rounded(cx.theme().radius_lg)
                .bg(cx.theme().muted)
                .text_sm()
                .child(if value.is_empty() {
                    "None".to_string()
                } else {
                    value.to_string()
                }),
        )
}

pub fn callout(title: &str, message: &str, cx: &App) -> Div {
    div()
        .p_3()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().danger)
        .bg(cx.theme().danger.opacity(0.08))
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .text_color(cx.theme().danger)
                        .child(title.to_string()),
                )
                .child(div().text_sm().child(message.to_string())),
        )
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

pub fn status_chip(status: QueryStatus, cx: &App) -> Div {
    let background = match status {
        QueryStatus::Success => cx.theme().success.opacity(0.12),
        QueryStatus::Failure => cx.theme().danger.opacity(0.12),
        QueryStatus::Cancelled => cx.theme().warning.opacity(0.12),
        QueryStatus::LoadingEmpty | QueryStatus::LoadingWithData => cx.theme().info.opacity(0.12),
        QueryStatus::Idle => cx.theme().muted,
    };
    chip(status.label(), background, cx)
}

pub fn status_dot(status: QueryStatus) -> &'static str {
    match status {
        QueryStatus::Idle => "[ ]",
        QueryStatus::LoadingEmpty | QueryStatus::LoadingWithData => "[~]",
        QueryStatus::Success => "[+]",
        QueryStatus::Failure => "[x]",
        QueryStatus::Cancelled => "!",
    }
}

pub fn empty_state(status: QueryStatus, cx: &App) -> Div {
    div()
        .p_5()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .text_color(cx.theme().muted_foreground)
        .child(match status {
            QueryStatus::LoadingEmpty => "Request is loading without cached data.",
            QueryStatus::Cancelled => "Request was cancelled before a response was applied.",
            _ => "No response captured for this tab yet.",
        })
}
