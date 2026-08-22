use gpui::prelude::*;
use gpui::*;

use gpui_component::{
    ActiveTheme as _, Disableable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
};

use gpui_query::core::{QueryResource, QueryStatus, RetryPolicy};

use super::super::QueryPlaygroundPage;
use super::super::ui_helpers::{chip, mini_card, section_card, status_badge};

// Helpers for the recurring `Option<(Entity<QueryResource<..>>, Subscription)>`
// read shape used across the playground render sections (audit finding D4).
// Each encapsulates the `read_with` + `map_or`/`and_then` boilerplate so call
// sites read as one-liners. Note: these only fit the QueryStatus/Option<T>
// accessors — MutationStatus, infinite `page_count`, and select bare-Entity
// sites deliberately stay inline.

/// Read the status of an optional query entity, defaulting to [`QueryStatus::Idle`].
fn query_status<T: 'static, E: 'static>(
    opt: Option<&(Entity<QueryResource<T, E>>, Subscription)>,
    cx: &App,
) -> QueryStatus {
    opt.map_or(QueryStatus::Idle, |(e, _)| {
        e.read_with(cx, |r, _| r.status())
    })
}

/// Read the cloned data (if any) of an optional query entity.
fn query_data<T: Clone + 'static, E: 'static>(
    opt: Option<&(Entity<QueryResource<T, E>>, Subscription)>,
    cx: &App,
) -> Option<T> {
    opt.and_then(|(e, _)| e.read_with(cx, |r, _| r.data().cloned()))
}

/// Read the cloned error (if any) of an optional query entity.
fn query_error<T: 'static, E: Clone + 'static>(
    opt: Option<&(Entity<QueryResource<T, E>>, Subscription)>,
    cx: &App,
) -> Option<E> {
    opt.and_then(|(e, _)| e.read_with(cx, |r, _| r.error().cloned()))
}

impl QueryPlaygroundPage {
    pub(in super::super) fn render_cache_policies(&mut self, cx: &mut Context<Self>) -> Div {
        // NoCache
        let nocache_status = query_status(self.nocache_query.as_ref(), cx);
        let nocache_loading = nocache_status.is_loading();

        // TTL
        let ttl_status = query_status(self.ttl_query.as_ref(), cx);
        let ttl_loading = ttl_status.is_loading();

        // SWR
        let swr_status = query_status(self.swr_query.as_ref(), cx);
        let swr_loading = swr_status.is_loading();

        section_card(
            "Cache Policies",
            "Compare NoCache, TTL 5s, and StaleWhileRevalidate 3s/7s.",
            cx,
        )
        .child(
            h_flex()
                .gap_4()
                .px_4()
                .py_3()
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

    pub(in super::super) fn render_request_policies(&mut self, cx: &mut Context<Self>) -> Div {
        let latest_status = query_status(self.latest_wins_query.as_ref(), cx);
        let latest_data = query_data(self.latest_wins_query.as_ref(), cx);
        let latest_loading = latest_status.is_loading();

        let ignore_status = query_status(self.ignore_query.as_ref(), cx);
        let ignore_data = query_data(self.ignore_query.as_ref(), cx);
        let ignore_loading = ignore_status.is_loading();

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

    pub(in super::super) fn render_retry_policy(&mut self, cx: &mut Context<Self>) -> Div {
        let status = query_status(self.retry_query.as_ref(), cx);
        let error = query_error(self.retry_query.as_ref(), cx);
        let loading = status.is_loading();
        // Read the actual policy off the entity so the chips stay truthful.
        let policy = self
            .retry_query
            .as_ref()
            .map_or(RetryPolicy::new(3).with_delay(400), |(e, _)| {
                e.read_with(cx, |r, _| r.retry_policy().clone())
            });

        section_card(
            "Retry Policy",
            "Fetcher always fails. The RetryPolicy retries 3× with backoff before \
             giving up — the button stays \"Retrying…\" through all attempts (that \
             delay IS the retries happening), then ends in Failure.",
            cx,
        )
        .child(
            h_flex().gap_2().flex_wrap().px_4().py_3().child(
                Button::new("pg-retry-trigger")
                    .primary()
                    .label(if loading {
                        "Retrying…"
                    } else {
                        "Trigger Failing Fetch"
                    })
                    .disabled(loading)
                    .on_click(cx.listener(|this, _, _, cx| this.trigger_failing_fetch(cx))),
            ),
        )
        .child(
            h_flex()
                .gap_3()
                .items_center()
                .px_4()
                .pb_3()
                .child(status_badge(status, cx))
                .child(chip(
                    &format!("max retries: {}", policy.max_retries),
                    cx.theme().background,
                    cx,
                ))
                .child(chip(
                    &format!("backoff: {}ms", policy.retry_delay_ms),
                    cx.theme().background,
                    cx,
                ))
                .when_some(error, |el, _| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .child(format!("Gave up after {} retries.", policy.max_retries)),
                    )
                }),
        )
    }
}
