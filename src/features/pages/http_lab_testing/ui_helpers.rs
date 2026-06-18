use std::time::Instant;

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _,
    button::{Button, ButtonVariants as _},
    v_flex,
};

use gpui_query_legacy::QueryResource;

use super::{RENDER_LOG, RawResponse};

pub(crate) fn section_card(title: &str, description: &str, cx: &App) -> Div {
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
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title.to_string()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(description.to_string()),
                        ),
                ),
        )
}

pub(crate) fn toggle_button(id: &str, label: &str, expanded: bool) -> Button {
    Button::new(id.to_string())
        .ghost()
        .label(if expanded {
            format!("Hide {label}")
        } else {
            format!("Show {label}")
        })
        .tooltip("Toggles heavy debug content for this section.")
}

pub(crate) fn preview_excerpt(value: &str, limit: usize) -> String {
    let truncated: String = value.chars().take(limit).collect();
    if value.chars().count() > limit {
        format!("{truncated}\n... truncated")
    } else {
        truncated
    }
}

pub(crate) fn compact_resource_preview(resource: Option<&RawResponse>) -> String {
    match resource {
        Some(response) => format!(
            "status={} bytes={} preview=\"{}\"",
            response.status,
            response.bytes,
            preview_excerpt(&response.preview, 96).replace('\n', " ")
        ),
        None => "none".to_string(),
    }
}

pub(crate) fn local_lab_history_panel(page: &super::HttpLabTestingPage, cx: &App) -> Div {
    let render_started = Instant::now();
    let mut body = v_flex().gap_1();
    for (action, response) in page.local_lab_history.iter().take(6) {
        body = body.child(div().text_xs().font_family("monospace").child(format!(
            "{} status={} bytes={} url={}",
            action.label(),
            response.status,
            response.bytes,
            response.final_url
        )));
    }

    let view = div()
        .p_3()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .child(if page.local_lab_history.is_empty() {
            v_flex().child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No local lab history."),
            )
        } else {
            body
        });

    tracing::debug!(
        target: RENDER_LOG,
        elapsed_us = render_started.elapsed().as_micros() as u64,
        history_len = page.local_lab_history.len(),
        rendered_rows = page.local_lab_history.len().min(6),
        "HTTP Lab Testing local lab history panel rendered"
    );

    view
}

pub(crate) fn query_resource_row(
    label: &str,
    resource: &QueryResource<RawResponse>,
    cx: &App,
) -> Div {
    let active = resource
        .active_request_id()
        .map(|id| id.label())
        .unwrap_or_else(|| "none".to_string());
    let data = if resource.data().is_some() {
        "data"
    } else {
        "no data"
    };
    row(
        label,
        &format!("{} active={} {}", resource.status().label(), active, data),
        cx,
    )
}

pub(crate) fn row(label: &str, value: &str, cx: &App) -> Div {
    div()
        .flex()
        .gap_3()
        .items_start()
        .child(
            div()
                .w(px(140.))
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(div().flex_1().text_sm().child(value.to_string()))
}
