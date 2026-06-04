use std::time::Instant;

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Selectable as _,
    button::{Button, ButtonVariants as _},
    v_flex,
};

use crate::http_lab::{self, HttpExchange, HttpLabAction, HttpLabState};
use gpui_query::{QueryResource, QueryStatus, RequestPolicy};

const RENDER_LOG: &str = "gpui_starter::http_lab::render";

pub struct HttpLabPage {
    _subscriptions: Vec<Subscription>,
}

impl HttpLabPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe_global_in::<HttpLabState>(window, |_, _, cx| {
            cx.notify();
        })];

        Self {
            _subscriptions: subscriptions,
        }
    }

    fn run_action(&mut self, action: HttpLabAction, cx: &mut Context<Self>) {
        tracing::info!(
            target: "gpui_starter::http_lab::ui",
            action = action.id(),
            "HTTP Lab action clicked"
        );
        tracing::info!(
            target: "gpui_starter::http_lab::ui",
            action = action.id(),
            "HTTP Lab scheduling GPUI entity task"
        );
        cx.spawn(async move |_this, cx| {
            tracing::info!(
                target: "gpui_starter::http_lab::ui",
                action = action.id(),
                "HTTP Lab GPUI entity task started"
            );

            let handle = match cx.update(|cx| http_lab::prepare_action(action, cx)) {
                Some(handle) => handle,
                None => {
                    tracing::info!(
                        target: "gpui_starter::http_lab::ui",
                        action = action.id(),
                        "HTTP Lab action deduplicated inside entity task"
                    );
                    return;
                }
            };

            tracing::info!(
                target: "gpui_starter::http_lab::ui",
                action = action.id(),
                "HTTP Lab action prepared inside entity task"
            );
            http_lab::execute_action(handle, cx).await;
            tracing::info!(
                target: "gpui_starter::http_lab::ui",
                action = action.id(),
                "HTTP Lab GPUI entity task finished"
            );
        })
        .detach();
        tracing::info!(
            target: "gpui_starter::http_lab::ui",
            action = action.id(),
            "HTTP Lab GPUI entity task scheduled"
        );
    }
}

impl Render for HttpLabPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = Instant::now();

        // Extract action bar data first — action_bar needs &mut Context for
        // cx.listener(), so it must be built outside any state borrow.
        let (active_count, action_meta) = http_lab::read_state(cx, |state| {
            let meta = HttpLabAction::all()
                .iter()
                .copied()
                .map(|action| {
                    let resource = state.resource(action);
                    (action, resource.is_loading(), resource.request_policy())
                })
                .collect::<Vec<_>>();
            (state.active_count(), meta)
        });
        let bar = action_bar_from_meta(active_count, action_meta, cx);

        // Now borrow state immutably via read_state for all read-only panels.
        // These helpers only need &App (not &mut Context), so the borrow is safe.
        let (history_len, transition_len, selected_action, selected_status, view) =
            http_lab::read_state(cx, |state| {
                let selected_resource = state.selected_resource();
                let selected_action = state.selected_action;
                let selected_status = selected_resource.status();
                let history_len = state.history.len();
                let transition_len = state.transition_log.len();

                let view = v_flex()
                    .min_h_full()
                    .p_6()
                    .gap_5()
                    .child(hero(&state, cx))
                    .child(bar)
                    .child(tab_bar(&state))
                    .child(resource_panel(
                        &state,
                        state.selected_action,
                        selected_resource,
                        cx,
                    ))
                    .child(activity_panel(&state, cx));

                (history_len, transition_len, selected_action, selected_status, view)
            });

        tracing::info!(
            target: RENDER_LOG,
            elapsed_us = render_started.elapsed().as_micros() as u64,
            selected_action = selected_action.id(),
            selected_status = selected_status.label(),
            active_count,
            history_len,
            transition_len,
            "HTTP Lab render completed"
        );

        view
    }
}

fn hero(state: &HttpLabState, cx: &App) -> Div {
    div()
        .p_5()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted)
        .child(
            v_flex()
                .gap_3()
                .child(
                    div()
                        .text_2xl()
                        .font_weight(FontWeight::BOLD)
                        .child("HTTP Lab"),
                )
                .child(
                    div()
                        .max_w(px(760.))
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("A small React Query-style GPUI store: every request type has its own resource state, request policy, cache policy, request id, cancellation guard, and response cache."),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(chip(
                            &format!("Active requests: {}", state.active_count()),
                            cx.theme().background,
                            cx,
                        ))
                        .child(chip(
                            &format!("History: {}", state.history.len()),
                            cx.theme().background,
                            cx,
                        ))
                        .child(chip(
                            "Cancellation is logical: blocking reqwest may finish, but stale request results are ignored.",
                            cx.theme().background,
                            cx,
                        )),
                ),
        )
}

/// Build the action bar from pre-extracted scalar data rather than a borrowed
/// `&HttpLabState`. This allows the caller to use `&mut Context` (needed for
/// `cx.listener()`) without holding an immutable borrow of the global state.
fn action_bar_from_meta(
    active_count: usize,
    action_meta: Vec<(HttpLabAction, bool, RequestPolicy)>,
    cx: &mut Context<HttpLabPage>,
) -> Div {
    div()
        .flex()
        .flex_wrap()
        .gap_2()
        .children(action_meta.into_iter().map(|(action, is_loading, policy)| {
            let blocks_duplicate = is_loading && policy == RequestPolicy::IgnoreWhileLoading;
            Button::new(format!("http-lab-run-{}", action.id()))
                .outline()
                .label(if is_loading {
                    format!("Loading {}", action.label())
                } else {
                    action.label().to_string()
                })
                .disabled(blocks_duplicate)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.run_action(action, cx);
                }))
        }))
        .child(
            Button::new("http-lab-cancel-all")
                .outline()
                .label("Cancel all")
                .disabled(active_count == 0)
                .on_click(|_, _, cx| {
                    http_lab::cancel_all(cx);
                }),
        )
        .child(
            Button::new("http-lab-reset")
                .outline()
                .label("Reset")
                .on_click(|_, _, cx| {
                    http_lab::reset(cx);
                }),
        )
}

fn tab_bar(state: &HttpLabState) -> Div {
    div()
        .flex()
        .flex_wrap()
        .gap_1()
        .children(HttpLabAction::all().iter().copied().map(|action| {
            let resource = state.resource(action);
            Button::new(format!("http-lab-tab-{}", action.id()))
                .ghost()
                .selected(state.selected_action == action)
                .label(format!(
                    "{} {}",
                    status_dot(resource.status()),
                    action.label()
                ))
                .on_click(move |_, _, cx| {
                    http_lab::select_action(action, cx);
                })
        }))
}

fn resource_panel(
    state: &HttpLabState,
    action: HttpLabAction,
    resource: &QueryResource<HttpExchange>,
    cx: &App,
) -> Div {
    let render_started = Instant::now();
    let has_data = resource.data().is_some();
    let has_error = resource.error().is_some();
    let active_request_id = resource.active_request_id().map(|id| id.label());

    let view = panel(action.label(), cx)
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(status_chip(resource.status(), cx))
                .child(chip(action.method_label(), cx.theme().background, cx))
                .child(chip(
                    &resource.cache_policy().label(),
                    cx.theme().background,
                    cx,
                ))
                .child(chip(
                    resource.request_policy().label(),
                    cx.theme().background,
                    cx,
                ))
                .when_some(resource.active_request_id(), |this, request_id| {
                    this.child(chip(
                        &format!("request {}", request_id.label()),
                        cx.theme().background,
                        cx,
                    ))
                }),
        )
        .child(resource_metrics(resource, cx))
        .when(resource.is_loading(), |this| {
            this.child(
                Button::new(format!("http-lab-cancel-{}", action.id()))
                    .danger()
                    .outline()
                    .label("Cancel request")
                    .on_click(move |_, _, cx| {
                        http_lab::cancel_action(action, cx);
                    }),
            )
        })
        .when_some(resource.error(), |this, error| {
            this.child(callout("Error", error.message(), cx))
        })
        .when_some(resource.data(), |this, exchange| {
            this.child(exchange_panel(exchange, cx))
        })
        .when(resource.data().is_none(), |this| {
            this.child(empty_state(resource.status(), cx))
        })
        .when(action == HttpLabAction::Cookies, |this| {
            this.when_some(state.cookies.as_ref(), |this, cookies| {
                this.child(
                    panel("Cookie jar", cx)
                        .child(kv(
                            "Set-Cookie",
                            cookies.set_cookie_header.as_deref().unwrap_or("None"),
                            cx,
                        ))
                        .child(kv(
                            "Echoed cookies",
                            cookies.echoed_cookies_json.as_deref().unwrap_or("None"),
                            cx,
                        )),
                )
            })
        });

    tracing::debug!(
        target: RENDER_LOG,
        elapsed_us = render_started.elapsed().as_micros() as u64,
        action = action.id(),
        status = resource.status().label(),
        has_data,
        has_error,
        active_request_id,
        "HTTP Lab resource panel rendered"
    );

    view
}

fn resource_metrics(resource: &QueryResource<HttpExchange>, cx: &App) -> Div {
    div()
        .grid()
        .gap_2()
        .child(kv("Cache hits", &resource.cache_hits().to_string(), cx))
        .child(kv("Cancelled", &resource.cancelled_count().to_string(), cx))
        .child(kv(
            "Ignored stale results",
            &resource.ignored_results().to_string(),
            cx,
        ))
        .child(kv(
            "Last update",
            &resource
                .last_updated_at_ms()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "Never".to_string()),
            cx,
        ))
}

fn activity_panel(state: &HttpLabState, cx: &App) -> Div {
    let render_started = Instant::now();
    let view = div()
        .grid()
        .gap_4()
        .child(
            panel("Lifecycle trace", cx).children(state.transition_log.iter().map(|entry| {
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(entry.clone())
            })),
        )
        .child(
            panel("History", cx).children(state.history.iter().take(10).map(|exchange| {
                let fields = http_lab::response_fields(exchange);
                let summary = fields
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>()
                    .join(" | ");
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(summary)
            })),
        );

    tracing::debug!(
        target: RENDER_LOG,
        elapsed_us = render_started.elapsed().as_micros() as u64,
        history_len = state.history.len(),
        transition_len = state.transition_log.len(),
        "HTTP Lab activity panel rendered"
    );

    view
}

fn exchange_panel(exchange: &HttpExchange, cx: &App) -> Div {
    let render_started = Instant::now();
    let mut panel = panel("Response", cx)
        .child(kv("Label", &exchange.label, cx))
        .child(kv("Method", &exchange.request.method, cx))
        .child(kv("URL", &exchange.request.url, cx))
        .child(kv(
            "Request body kind",
            exchange.request.request_body_kind.label(),
            cx,
        ))
        .child(preview_block(
            "Request body preview",
            &exchange.request.request_body_preview,
            cx,
        ));

    if let Some(response) = &exchange.response {
        panel = panel
            .child(kv(
                "Status",
                &format!("{} {}", response.status, response.status_text),
                cx,
            ))
            .child(kv("Final URL", &response.final_url, cx))
            .child(kv("Elapsed", &format!("{}ms", response.elapsed_ms), cx))
            .child(kv("Body kind", response.body_kind.label(), cx))
            .child(headers_block(&response.headers, cx))
            .child(preview_block("Body text", &response.body_preview, cx))
            .when_some(response.parsed_json.as_ref(), |this, json| {
                this.child(preview_block("Parsed JSON", json, cx))
            })
            .when_some(response.parsed_xml_preview.as_ref(), |this, xml| {
                this.child(preview_block("Parsed XML", xml, cx))
            });
    }

    let view = panel.when_some(exchange.error.as_ref(), |this, error| {
        this.child(callout("Response error", error, cx))
    });

    tracing::debug!(
        target: RENDER_LOG,
        elapsed_us = render_started.elapsed().as_micros() as u64,
        label = %exchange.label,
        has_response = exchange.response.is_some(),
        has_error = exchange.error.is_some(),
        request_url = %exchange.request.url,
        "HTTP Lab exchange panel rendered"
    );

    view
}

fn headers_block(headers: &[(String, String)], cx: &App) -> Div {
    panel("Headers", cx).children(
        headers
            .iter()
            .take(16)
            .map(|(name, value)| kv(name, value, cx)),
    )
}

fn empty_state(status: QueryStatus, cx: &App) -> Div {
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

fn panel(title: &str, cx: &App) -> Div {
    v_flex()
        .gap_3()
        .p_4()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .child(section_title(title))
}

fn section_title(title: &str) -> Div {
    div()
        .text_lg()
        .font_weight(FontWeight::BOLD)
        .child(title.to_string())
}

fn kv(label: &str, value: &str, cx: &App) -> Div {
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

fn preview_block(label: &str, value: &str, cx: &App) -> Div {
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

fn callout(title: &str, message: &str, cx: &App) -> Div {
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

fn chip(label: &str, background: Hsla, cx: &App) -> Div {
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

fn status_chip(status: QueryStatus, cx: &App) -> Div {
    let background = match status {
        QueryStatus::Success => cx.theme().success.opacity(0.12),
        QueryStatus::Failure => cx.theme().danger.opacity(0.12),
        QueryStatus::Cancelled => cx.theme().warning.opacity(0.12),
        QueryStatus::LoadingEmpty | QueryStatus::LoadingWithData => cx.theme().info.opacity(0.12),
        QueryStatus::Idle => cx.theme().muted,
    };
    chip(status.label(), background, cx)
}

fn status_dot(status: QueryStatus) -> &'static str {
    match status {
        QueryStatus::Idle => "[ ]",
        QueryStatus::LoadingEmpty | QueryStatus::LoadingWithData => "[~]",
        QueryStatus::Success => "[+]",
        QueryStatus::Failure => "[x]",
        QueryStatus::Cancelled => "!",
    }
}
