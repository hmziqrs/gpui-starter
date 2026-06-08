use gpui::prelude::*;
use gpui::*;

use gpui_component::{
    ActiveTheme as _,
    Disableable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
    input::Input,
};

use gpui_query_v2::core::{
    MutationStatus, QueryStatus, RetryPolicy,
};

use crate::ui::widgets::{render_virtual_list, uniform_item_sizes};

use super::{PlaygroundPage, QueryPlaygroundPage};
use super::ui_helpers::{section_card, mini_card, status_badge, chip, source_preview, mapped_preview};

// ---------------------------------------------------------------------------
// Section renderers
// ---------------------------------------------------------------------------

impl QueryPlaygroundPage {
    pub(super) fn render_simple_query(&mut self, cx: &mut Context<Self>) -> Div {
        let loading = self.simple_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });
        let status = self.simple_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let data_preview = self.simple_query.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.data().cloned())
        });
        let cache_age = self.simple_query.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                r.last_updated_at_ms().map(|t| now.saturating_sub(t))
            })
        });
        let bg = cx.theme().background;

        section_card("Simple Query", "Basic fetch with NoCache + LatestWins. Simulates 1s async work.", cx)
            .child(
                h_flex().gap_2().flex_wrap().px_4().py_3()
                    .child(
                        Button::new("pg-simple-fetch")
                            // Finding 11: Use .primary() for the main action button.
                            .primary()
                            .label(if loading { "Fetching..." } else { "Fetch" })
                            .disabled(loading)
                            .on_click(cx.listener(|this, _, _, cx| this.fetch_simple(cx))),
                    )
                    .child(
                        Button::new("pg-simple-cancel")
                            .outline()
                            .label("Cancel")
                            .disabled(!loading)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_simple(cx))),
                    )
                    .child(
                        Button::new("pg-simple-reset")
                            .outline()
                            .label("Reset")
                            .on_click(cx.listener(|this, _, _, cx| this.reset_simple(cx))),
                    ),
            )
            .child(
                h_flex().gap_3().items_center().px_4().pb_3()
                    .child(status_badge(status, cx))
                    .when_some(data_preview, |el, user| {
                        el.child(chip(&format!("{} <{}>", user.name, user.email), bg, cx))
                    })
                    .when_some(cache_age, |el, age| {
                        el.child(chip(&format!("age: {}ms", age), bg, cx))
                    }),
            )
    }

    pub(super) fn render_cache_policies(&mut self, cx: &mut Context<Self>) -> Div {

        // NoCache
        let nocache_status = self.nocache_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let nocache_loading = self.nocache_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });

        // TTL
        let ttl_status = self.ttl_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let ttl_loading = self.ttl_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });

        // SWR
        let swr_status = self.swr_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let swr_loading = self.swr_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });

        section_card("Cache Policies", "Compare NoCache, TTL 5s, and StaleWhileRevalidate 3s/7s.", cx)
            .child(
                h_flex().gap_4().px_4().py_3()
                    // NoCache card
                    .child(
                        mini_card("NoCache", cx)
                            .child(status_badge(nocache_status, cx))
                            .child(
                                Button::new("pg-nocache-fetch")
                                    .primary()
                                    .label(if nocache_loading { "Fetching" } else { "Fetch" })
                                    .disabled(nocache_loading)
                                    .on_click(cx.listener(|this, _, _, cx| this.fetch_nocache(cx))),
                            ),
                    )
                    // TTL card
                    .child(
                        mini_card("TTL 5s", cx)
                            .child(status_badge(ttl_status, cx))
                            .child(
                                Button::new("pg-ttl-fetch")
                                    .primary()
                                    .label(if ttl_loading { "Fetching" } else { "Fetch" })
                                    .disabled(ttl_loading)
                                    .on_click(cx.listener(|this, _, _, cx| this.fetch_ttl(cx))),
                            ),
                    )
                    // SWR card
                    .child(
                        mini_card("SWR 3s/7s", cx)
                            .child(status_badge(swr_status, cx))
                            .child(
                                Button::new("pg-swr-fetch")
                                    .primary()
                                    .label(if swr_loading { "Fetching" } else { "Fetch" })
                                    .disabled(swr_loading)
                                    .on_click(cx.listener(|this, _, _, cx| this.fetch_swr(cx))),
                            ),
                    ),
            )
    }

    pub(super) fn render_request_policies(&mut self, cx: &mut Context<Self>) -> Div {

        let latest_status = self.latest_wins_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let latest_data = self.latest_wins_query.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.data().cloned())
        });
        let latest_loading = self.latest_wins_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });

        let ignore_status = self.ignore_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let ignore_data = self.ignore_query.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.data().cloned())
        });
        let ignore_loading = self.ignore_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });

        section_card(
            "Request Policies",
            "LatestWins: last fetch wins, older results discarded. IgnoreWhileLoading: first fetch completes, rest ignored.",
            cx,
        )
        .child(
            h_flex().gap_4().px_4().py_3()
                // LatestWins card
                .child(
                    mini_card("LatestWins", cx)
                        .child(status_badge(latest_status, cx))
                        .when_some(latest_data, |el, d| {
                            el.child(chip(&d, cx.theme().background, cx))
                        })
                        .child(
                            Button::new("pg-latest-spam")
                                .primary()
                                .label("Spam Fetch (5x)")
                                .disabled(latest_loading)
                                .on_click(cx.listener(|this, _, _, cx| this.spam_latest_wins(cx))),
                        ),
                )
                // IgnoreWhileLoading card
                .child(
                    mini_card("IgnoreWhileLoading", cx)
                        .child(status_badge(ignore_status, cx))
                        .when_some(ignore_data, |el, d| {
                            el.child(chip(&d, cx.theme().background, cx))
                        })
                        .child(
                            Button::new("pg-ignore-spam")
                                .primary()
                                .label("Spam Fetch (5x)")
                                .disabled(ignore_loading)
                                .on_click(cx.listener(|this, _, _, cx| this.spam_ignore(cx))),
                        ),
                ),
        )
    }

    pub(super) fn render_retry_policy(&mut self, cx: &mut Context<Self>) -> Div {

        let status = self.retry_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let retry_count = self.retry_query.as_ref().map_or(0, |(e, _)| {
            e.read_with(cx, |r, _| r.retry_count())
        });
        let loading = status.is_loading();
        let policy = RetryPolicy::new(3).with_delay(200);

        section_card(
            "Retry Policy",
            "Fetcher always returns Err. Shows retry count incrementing with exponential backoff (3 retries, 200ms base).",
            cx,
        )
        .child(
            h_flex().gap_2().flex_wrap().px_4().py_3()
                .child(
                    Button::new("pg-retry-trigger")
                        .primary()
                        .label(if loading { "Retrying..." } else { "Trigger Failing Fetch" })
                        .disabled(loading)
                        .on_click(cx.listener(|this, _, _, cx| this.trigger_failing_fetch(cx))),
                ),
        )
        .child(
            h_flex().gap_3().items_center().px_4().pb_3()
                .child(status_badge(status, cx))
                .child(chip(&format!("retries: {}/{}", retry_count, policy.max_retries), cx.theme().background, cx))
                .child(chip(&format!("backoff: {}ms base", policy.retry_delay_ms), cx.theme().background, cx)),
        )
    }

    pub(super) fn render_mutation(&mut self, cx: &mut Context<Self>) -> Div {

        let m_status = self.mutation_entity.as_ref().map_or(
            MutationStatus::Idle,
            |(e, _)| e.read_with(cx, |r, _| r.status()),
        );
        let m_data = self.mutation_entity.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.data().cloned())
        });
        let m_error = self.mutation_entity.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.error().cloned())
        });
        let m_vars = self.mutation_entity.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.variables().cloned())
        });
        let m_loading = m_status == MutationStatus::Loading;

        let status_color = match m_status {
            MutationStatus::Idle => cx.theme().muted_foreground,
            MutationStatus::Loading => cx.theme().info,
            MutationStatus::Success => cx.theme().success,
            MutationStatus::Failure => cx.theme().danger,
        };

        section_card(
            "Mutation",
            "Text input for mutation variables. Mutate() fires async; mutate_with_callbacks() also logs on_success/on_error.",
            cx,
        )
        .child(
            h_flex().gap_2().items_center().px_4().py_2()
                // Finding 1/8: Replace static div with a proper editable Input
                // component that binds to mutation_input_state.
                .child(
                    div().min_w(px(200.))
                        .child(Input::new(&self.mutation_input_state))
                )
                .child(
                    Button::new("pg-mutate")
                        .primary()
                        .label(if m_loading { "Mutating..." } else { "Mutate" })
                        .disabled(m_loading)
                        .on_click(cx.listener(|this, _, _, cx| this.do_mutate(cx))),
                )
                .child(
                    Button::new("pg-mutate-cb")
                        .outline()
                        .label("Mutate with Callbacks")
                        .disabled(m_loading)
                        .on_click(cx.listener(|this, _, _, cx| this.do_mutate_with_callbacks(cx))),
                )
                .child(
                    Button::new("pg-mutate-reset")
                        .outline()
                        .label("Reset")
                        .on_click(cx.listener(|this, _, _, cx| this.reset_mutation(cx))),
                ),
        )
        .child(
            v_flex().gap_1().px_4().pb_3()
                .child(
                    h_flex().gap_3().items_center()
                        .child(
                            div()
                                .px_3()
                                .py_1()
                                .rounded(cx.theme().radius_lg)
                                .border_1()
                                .border_color(status_color)
                                .text_sm()
                                .text_color(status_color)
                                .child(m_status.label().to_string()),
                        )
                        .when_some(m_data, |el, d| {
                            el.child(chip(&format!("data: {}", d), cx.theme().background, cx))
                        })
                        .when_some(m_vars, |el, v| {
                            el.child(chip(&format!("vars: {}", v), cx.theme().background, cx))
                        })
                        .when_some(m_error, |el, e| {
                            el.child(
                                div()
                                    .px_3()
                                    .py_1()
                                    .rounded(cx.theme().radius_lg)
                                    .border_1()
                                    .border_color(cx.theme().danger)
                                    .text_sm()
                                    .text_color(cx.theme().danger)
                                    .child(format!("error: {}", e)),
                            )
                        }),
                ),
        )
    }

    pub(super) fn render_infinite_query(&mut self, cx: &mut Context<Self>) -> Div {

        let page_count = self.infinite_entity.as_ref().map_or(0, |(e, _)| {
            e.read_with(cx, |r, _| r.page_count())
        });
        let inf_status = self.infinite_entity.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let has_next = self.infinite_entity.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.has_next_page())
        });
        let has_prev = self.infinite_entity.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.has_previous_page())
        });
        let loading = inf_status.is_loading();
        let pages: Vec<(usize, PlaygroundPage)> = self.infinite_entity.as_ref()
            .map(|(e, _)| {
                let mut result = Vec::new();
                e.read_with(cx, |r, _| {
                    for (i, page) in r.pages().iter().enumerate() {
                        result.push((i, page.clone()));
                    }
                });
                result
            })
            .unwrap_or_default();

        section_card(
            "Infinite Query",
            "Paginated data with max_pages=3 so eviction is visible. Fetcher generates pages of 5 items each.",
            cx,
        )
        .child(
            h_flex().gap_2().flex_wrap().px_4().py_3()
                .child(
                    Button::new("pg-inf-next")
                        .primary()
                        .label("Load Next Page")
                        .disabled(loading || !has_next)
                        .on_click(cx.listener(|this, _, _, cx| this.load_next_page(cx))),
                )
                .child(
                    Button::new("pg-inf-prev")
                        .outline()
                        .label("Load Previous Page")
                        // Finding 3: Disable when has_prev is false, mirroring
                        // the 'Load Next Page' button's !has_next check.
                        .disabled(loading || !has_prev)
                        .on_click(cx.listener(|this, _, _, cx| this.load_prev_page(cx))),
                )
                .child(
                    Button::new("pg-inf-reset")
                        .outline()
                        .label("Reset")
                        .on_click(cx.listener(|this, _, _, cx| this.reset_infinite(cx))),
                )
                .child(status_badge(inf_status, cx))
                .child(chip(&format!("pages: {}/3 (max)", page_count), cx.theme().background, cx))
                .child(chip(&format!("has_next: {}", has_next), cx.theme().background, cx))
                .child(chip(&format!("has_prev: {}", has_prev), cx.theme().background, cx)),
        )
        .when(!pages.is_empty(), |el| {
            el.child(
                v_flex().gap_2().px_4().pb_3()
                    .children(pages.into_iter().map(|(idx, page)| {
                        div()
                            .px_3()
                            .py_2()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().muted)
                            .child(
                                v_flex().gap_1()
                                    .child(
                                        div().text_xs().font_weight(FontWeight::SEMIBOLD)
                                            .child(format!("Page {} (index {})", page.page_number, idx)),
                                    )
                                    .child(
                                        div().text_xs().text_color(cx.theme().muted_foreground)
                                            .child(page.items.join(", ")),
                                    ),
                            )
                    })),
            )
        })
    }

    pub(super) fn render_select_transform(&mut self, cx: &mut Context<Self>) -> Div {

        let source_data = self.select_source.as_ref().and_then(|e| {
            e.read_with(cx, |r, _| r.data().cloned())
        });
        let mapped_data = self.select_mapped.as_ref().and_then(|e| {
            e.read_with(cx, |r, _| r.data())
        });
        let source_status = self.select_source.as_ref().map_or(QueryStatus::Idle, |e| {
            e.read_with(cx, |r, _| r.status())
        });
        let loading = source_status.is_loading();

        section_card(
            "Select Transform",
            "Source query returns Vec<PlaygroundUser>. Transform projects to Vec<String> (names only).",
            cx,
        )
        .child(
            h_flex().gap_2().px_4().py_3()
                .child(
                    Button::new("pg-select-fetch")
                        .primary()
                        .label(if loading { "Fetching..." } else { "Fetch Source" })
                        .disabled(loading)
                        .on_click(cx.listener(|this, _, _, cx| this.fetch_select(cx))),
                )
                .child(status_badge(source_status, cx)),
        )
        .child(
            h_flex().gap_4().px_4().pb_3()
                .child(
                    v_flex().gap_1()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Source (Vec<User>)"))
                        .child(
                            div()
                                .p_2()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().muted)
                                .text_sm()
                                .child(source_preview(&source_data)),
                        ),
                )
                .child(
                    v_flex().gap_1()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Mapped (Vec<String>)"))
                        .child(
                            div()
                                .p_2()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().muted)
                                .text_sm()
                                .child(mapped_preview(&mapped_data)),
                        ),
                ),
        )
    }

    pub(super) fn render_imperative_fetch(&mut self, cx: &mut Context<Self>) -> Div {

        let status = self.imperative_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let data = self.imperative_query.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.data().cloned())
        });
        let signal_cancelled = self.imperative_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| {
                r.signal().map(|s| s.is_cancelled()).unwrap_or(false)
            })
        });
        let loading = status.is_loading();

        section_card(
            "Imperative Fetch",
            "Manual refetch with signal. Cancel mid-flight to observe cooperative cancellation.",
            cx,
        )
        .child(
            h_flex().gap_2().flex_wrap().px_4().py_3()
                .child(
                    Button::new("pg-imp-fetch")
                        .primary()
                        .label(if loading { "Fetching..." } else { "Fetch" })
                        .disabled(loading)
                        .on_click(cx.listener(|this, _, _, cx| this.fetch_imperative(cx))),
                )
                .child(
                    Button::new("pg-imp-cancel")
                        .outline()
                        .label("Cancel mid-flight")
                        .disabled(!loading)
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_imperative(cx))),
                )
                .child(
                    Button::new("pg-imp-reset")
                        .outline()
                        .label("Reset")
                        .on_click(cx.listener(|this, _, _, cx| this.reset_imperative(cx))),
                ),
        )
        .child(
            h_flex().gap_3().items_center().px_4().pb_3()
                .child(status_badge(status, cx))
                .when_some(data, |el, d| {
                    el.child(chip(&d, cx.theme().background, cx))
                })
                .child(chip(
                    &format!("signal cancelled: {}", signal_cancelled),
                    cx.theme().background,
                    cx,
                )),
        )
    }

    pub(super) fn render_activity_log(&self, cx: &mut Context<Self>) -> Div {
        let has_logs = !self.activity_log.is_empty();
        let log_count = self.activity_log.len();
        let scroll_handle = self.log_scroll_handle.clone();

        let card = section_card("Activity Log", "Tracks user actions across all sections.", cx)
            .child(
                h_flex().justify_between().items_center().px_4().py_1()
                    .child(
                        div().text_xs().text_color(cx.theme().muted_foreground)
                            .child(format!("{} entries", log_count))
                    )
                    .when(has_logs, |el| {
                        el.child(
                            Button::new("clear-logs")
                                .label("Clear Logs")
                                .compact()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.activity_log.clear();
                                    cx.notify();
                                }))
                        )
                    })
            );

        if self.activity_log.is_empty() {
            card.child(
                div().px_4().pb_3()
                    .child(
                        div().text_sm().text_color(cx.theme().muted_foreground)
                            .child("No activity yet. Click a button above."),
                    )
            )
        } else {
            // Virtualized list: only renders visible entries (20px each).
            let item_count = self.activity_log.len();
            let item_height = px(20.);
            let item_sizes = uniform_item_sizes(item_count, item_height);

            card.child(
                div().px_4().max_h(px(200.))
                    // Block scroll events from bubbling to the outer page scroll.
                    .on_scroll_wheel(|_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        render_virtual_list(
                            cx,
                            "activity-log-vlist",
                            item_sizes,
                            px(200.),
                            px(0.),
                            &scroll_handle,
                            false,
                            move |this: &mut Self, visible_range, _window, cx| {
                                // Render entries in reverse (newest first).
                                let total = this.activity_log.len();
                                visible_range.map(|ix| {
                                    let entry_ix = total - 1 - ix;
                                    let entry = this.activity_log.get(entry_ix).cloned().unwrap_or_default();
                                    div()
                                        .h(px(20.))
                                        .text_xs()
                                        .font_family("monospace")
                                        .text_color(cx.theme().muted_foreground)
                                        .child(entry)
                                        .into_any_element()
                                }).collect()
                            },
                        )
                    )
            )
        }
    }
}
