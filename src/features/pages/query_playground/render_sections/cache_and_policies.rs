use gpui::prelude::*;
use gpui::*;

use gpui_component::{
    ActiveTheme as _, Disableable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
};

use gpui_query::core::{QueryStatus, RetryPolicy};

use super::super::QueryPlaygroundPage;
use super::super::ui_helpers::{chip, mini_card, section_card, status_badge};

impl QueryPlaygroundPage {
    pub(in super::super) fn render_cache_policies(&mut self, cx: &mut Context<Self>) -> Div {
        // NoCache
        let nocache_status = self
            .nocache_query
            .as_ref()
            .map_or(QueryStatus::Idle, |(e, _)| {
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
        let latest_status = self
            .latest_wins_query
            .as_ref()
            .map_or(QueryStatus::Idle, |(e, _)| {
                e.read_with(cx, |r, _| r.status())
            });
        let latest_data = self
            .latest_wins_query
            .as_ref()
            .and_then(|(e, _)| e.read_with(cx, |r, _| r.data().cloned()));
        let latest_loading = self.latest_wins_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });

        let ignore_status = self
            .ignore_query
            .as_ref()
            .map_or(QueryStatus::Idle, |(e, _)| {
                e.read_with(cx, |r, _| r.status())
            });
        let ignore_data = self
            .ignore_query
            .as_ref()
            .and_then(|(e, _)| e.read_with(cx, |r, _| r.data().cloned()));
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

    pub(in super::super) fn render_retry_policy(&mut self, cx: &mut Context<Self>) -> Div {
        let status = self
            .retry_query
            .as_ref()
            .map_or(QueryStatus::Idle, |(e, _)| {
                e.read_with(cx, |r, _| r.status())
            });
        // The crate resets `retry_count` to 0 on terminal failure, so display
        // the high-water mark tracked by the observer in `ensure_retry_query`.
        let retry_count = self.retry_peak;
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
}
