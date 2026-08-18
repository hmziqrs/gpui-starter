use gpui::prelude::*;
use gpui::*;

use gpui_component::{ActiveTheme as _, Disableable as _, button::Button, h_flex};

use gpui_query::core::QueryStatus;

use super::super::ui_helpers::{chip, section_card, status_badge};
use super::super::{HttpFetchKind, QueryPlaygroundPage};

impl QueryPlaygroundPage {
    pub(in super::super) fn render_http_fetching(&mut self, cx: &mut Context<Self>) -> Div {
        let loading = self.http_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });
        let status = self
            .http_query
            .as_ref()
            .map_or(QueryStatus::Idle, |(e, _)| {
                e.read_with(cx, |r, _| r.status())
            });
        let result = self
            .http_query
            .as_ref()
            .and_then(|(e, _)| e.read_with(cx, |r, _| r.data().cloned()));
        let error = self
            .http_query
            .as_ref()
            .and_then(|(e, _)| e.read_with(cx, |r, _| r.error().cloned()));

        let theme = cx.theme();
        let bg = theme.background;
        let muted = theme.muted;
        let radius_lg = theme.radius_lg;
        let danger = theme.danger;
        let _ = theme;

        section_card(
            "HTTP Fetching",
            "Real network requests via reqwest (httpbin.org), bridged onto the tokio runtime. \
             LatestWins cancels in-flight on a new click.",
            cx,
        )
        .child(
            h_flex()
                .gap_2()
                .flex_wrap()
                .px_4()
                .py_3()
                .child(
                    Button::new("pg-http-json")
                        .outline()
                        .label("GET JSON")
                        .disabled(loading)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.fetch_http(HttpFetchKind::GetJson, cx)
                        })),
                )
                .child(
                    Button::new("pg-http-xml")
                        .outline()
                        .label("GET XML")
                        .disabled(loading)
                        .on_click(
                            cx.listener(|this, _, _, cx| {
                                this.fetch_http(HttpFetchKind::GetXml, cx)
                            }),
                        ),
                )
                .child(
                    Button::new("pg-http-text")
                        .outline()
                        .label("GET text")
                        .disabled(loading)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.fetch_http(HttpFetchKind::GetText, cx)
                        })),
                )
                .child(
                    Button::new("pg-http-post")
                        .outline()
                        .label("POST JSON")
                        .disabled(loading)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.fetch_http(HttpFetchKind::PostJson, cx)
                        })),
                )
                .child(
                    Button::new("pg-http-fail")
                        .outline()
                        .label("GET fail")
                        .disabled(loading)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.fetch_http(HttpFetchKind::GetFail, cx)
                        })),
                )
                .child(
                    Button::new("pg-http-reset")
                        .outline()
                        .label("Reset")
                        .on_click(cx.listener(|this, _, _, cx| this.reset_http(cx))),
                ),
        )
        .child(
            h_flex()
                .gap_3()
                .items_center()
                .px_4()
                .pb_3()
                .child(status_badge(status, cx))
                .when_some(result.as_ref(), |el, r| {
                    el.child(chip(
                        &format!("{} {} → {}", r.status, r.method, r.url),
                        bg,
                        cx,
                    ))
                    .child(chip(
                        &format!(
                            "{} · {}ms",
                            short_content_type(&r.content_type),
                            r.elapsed_ms
                        ),
                        bg,
                        cx,
                    ))
                }),
        )
        .when_some(result.as_ref(), |el, r| {
            if r.body.is_empty() {
                el
            } else {
                el.child(
                    div()
                        .id("pg-http-body")
                        .mx_4()
                        .mb_4()
                        .p_3()
                        .rounded(radius_lg)
                        .bg(muted)
                        .max_h(px(280.))
                        .overflow_y_scroll()
                        .text_xs()
                        .child(r.body.clone()),
                )
            }
        })
        .when_some(error.as_ref(), |el, err| {
            el.child(
                div()
                    .mx_4()
                    .mb_4()
                    .p_3()
                    .text_xs()
                    .text_color(danger)
                    .child(err.to_string()),
            )
        })
    }
}

/// Strip the `; charset=…` suffix from a content-type header for compact display.
fn short_content_type(ct: &str) -> &str {
    ct.split(';').next().unwrap_or(ct).trim()
}
