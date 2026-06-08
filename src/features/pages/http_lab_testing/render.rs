use std::time::Instant;

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _,
    Disableable as _,
    button::{Button, ButtonVariants as _},
    v_flex,
};


use crate::services::http_lab::HttpLabAction;

use super::{
    ui_helpers::{
        compact_resource_preview, local_lab_history_panel, preview_excerpt, query_resource_row,
        row, section_card, toggle_button,
    }, RENDER_LOG, RawStatus,
};

impl Render for super::HttpLabTestingPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = Instant::now();
        let is_sending = matches!(self.status, RawStatus::Sending);
        let active_operation_id = self.active_operation_id;
        let query_status = self.query_resource.status();
        let local_selected = self.local_lab_selected;
        let local_history_len = self.local_lab_history.len();

        // -- Section 1: Query Lifecycle --
        let query_lifecycle_section = section_card(
            "Query Lifecycle",
            "Test request policies (LatestWins, IgnoreWhileLoading) and cache TTL behavior",
            cx,
        )
        .child(
            div().flex().flex_wrap().gap_2().px_4().py_3()
            .child(
                Button::new("http-lab-testing-query-send")
                    .outline()
                    .label(if is_sending {
                        "Sending query GET"
                    } else {
                        "Send query GET"
                    })
                    .disabled(is_sending)
                    .tooltip("Real HTTP GET through QueryResource (NoCache, LatestWins). Result appears in the resource state below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send_query_get(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-query-ttl")
                    .outline()
                    .label("Query TTL")
                    .disabled(is_sending)
                    .tooltip("Sync probe: start\u{2192}complete\u{2192}start again. Second should hit cache. Look for cache_hit=true below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_ttl_cache(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-query-ignore")
                    .outline()
                    .label("Query ignore")
                    .disabled(is_sending)
                    .tooltip("Sync probe: start request, try duplicate. Duplicate ignored. Look for duplicate_ignored=true below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_ignore_while_loading(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-query-latest")
                    .outline()
                    .label("Query latest")
                    .disabled(is_sending)
                    .tooltip("Sync probe: two requests, second replaces first. Stale completion rejected. Look for stale_accepted=false below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_latest_wins(cx);
                    })),
            ),
        )
        .child(
            div().px_4().pb_3().child(
                toggle_button(
                    "http-lab-testing-toggle-query-details",
                    "Query details",
                    self.show_query_details,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_query_details(cx))),
            ),
        )
        .when(self.show_query_details, |section| {
            section.child(
                div().px_4().py_3().child(
                    v_flex().gap_2()
                        .child(query_resource_row("Main", &self.query_resource, cx))
                        .child(query_resource_row("TTL", &self.query_ttl_resource, cx))
                        .child(query_resource_row("Ignore", &self.query_ignore_resource, cx))
                        .child(query_resource_row("Latest", &self.query_latest_resource, cx))
                        .child(row("Query message", &self.query_message, cx)),
                ),
            )
        });

        // -- Section 2: Cancel Signal --
        let signal_resource = &self.query_signal_resource;
        let signal_status = match signal_resource.signal() {
            Some(signal) => {
                if signal.is_cancelled() {
                    "cancelled"
                } else {
                    "active"
                }
            }
            None => "none",
        };
        let cancel_signal_section = section_card(
            "Cancel Signal",
            "Test cooperative cancellation signal that propagates to cloned signal references",
            cx,
        )
        .child(
            div().flex().flex_wrap().gap_2().px_4().py_3()
            .child(
                Button::new("http-lab-testing-query-signal")
                    .outline()
                    .label("Query signal")
                    .disabled(is_sending)
                    .tooltip("Sync probe: begin request, clone signal, cancel resource. Cloned signal should read is_cancelled=true. Look for before=false after=true below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_signal(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-signal-cancel")
                    .danger()
                    .outline()
                    .label("Cancel active")
                    .disabled(!is_sending)
                    .tooltip("Cancels any in-flight request via CancellationToken.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel(cx);
                    })),
            ),
        )
        .child(
            div().px_4().pb_3().child(
                toggle_button(
                    "http-lab-testing-toggle-signal-details",
                    "Signal details",
                    self.show_signal_details,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_signal_details(cx))),
            ),
        )
        .when(self.show_signal_details, |section| {
            section.child(
                div().px_4().py_3().child(
                    v_flex().gap_2()
                        .child(query_resource_row("Signal resource", signal_resource, cx))
                        .child(row("Signal", signal_status, cx))
                        .child(row("Signal message", &self.query_signal_message, cx)),
                ),
            )
        });

        // -- Section 3: Cache & Data Retention --
        let placeholder_resource = &self.query_placeholder_resource;
        let ph_data = compact_resource_preview(placeholder_resource.data());
        let ph_placeholder = compact_resource_preview(placeholder_resource.placeholder_data());
        let ph_display = compact_resource_preview(placeholder_resource.display_data());
        let ph_previous = compact_resource_preview(placeholder_resource.previous_data());

        let data_retention_section = section_card(
            "Cache & Data Retention",
            "Test placeholder data fallback, automatic previous_data tracking on success, and rollback",
            cx,
        )
        .child(
            div().flex().flex_wrap().gap_2().px_4().py_3()
            .child(
                Button::new("http-lab-testing-placeholder")
                    .outline()
                    .label("Placeholder data")
                    .disabled(is_sending)
                    .tooltip("Sync probe: seed data, set placeholder, reset, begin loading. display_data returns placeholder during loading, real data after completion. Check results below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_placeholder_data(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-previous-data")
                    .outline()
                    .label("Previous data")
                    .disabled(is_sending)
                    .tooltip("Sync probe: seed 'first', then 'second'. previous_data holds 'first'. Check results below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_previous_data(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-rollback")
                    .outline()
                    .label("Rollback")
                    .disabled(is_sending)
                    .tooltip("Sync probe: seed data, overwrite, rollback_to_previous. Data restored. Check results below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_rollback(cx);
                    })),
            ),
        )
        .child(
            div().px_4().pb_3().child(
                toggle_button(
                    "http-lab-testing-toggle-retention-details",
                    "Retention details",
                    self.show_retention_details,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_retention_details(cx))),
            ),
        )
        .when(self.show_retention_details, |section| {
            section.child(
                div().px_4().py_3().child(
                    v_flex().gap_2()
                        .child(query_resource_row("Placeholder resource", placeholder_resource, cx))
                        .child(row("Data", &ph_data, cx))
                        .child(row("Placeholder", &ph_placeholder, cx))
                        .child(row("Display data", &ph_display, cx))
                        .child(row("Previous data", &ph_previous, cx))
                        .child(row("Placeholder message", &self.query_placeholder_message, cx)),
                ),
            )
        });

        // -- Section 4: Optimistic Updates --
        let optimistic_resource = &self.query_optimistic_resource;
        let opt_data = compact_resource_preview(optimistic_resource.data());
        let opt_previous = compact_resource_preview(optimistic_resource.previous_data());
        let opt_display = compact_resource_preview(optimistic_resource.display_data());
        let opt_status = optimistic_resource.status().label().to_string();

        let optimistic_section = section_card(
            "Optimistic Updates",
            "Test optimistic writes that store previous data for rollback on mutation failure",
            cx,
        )
        .child(
            div().flex().flex_wrap().gap_2().px_4().py_3()
            .child(
                Button::new("http-lab-testing-optimistic-set")
                    .outline()
                    .label("Optimistic set")
                    .disabled(is_sending)
                    .tooltip("Sync probe: seed 'original', set_data('optimistic'). data='optimistic' previous='original'. Status unchanged.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_optimistic_set(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-optimistic-rollback")
                    .outline()
                    .label("Optimistic rollback")
                    .disabled(is_sending)
                    .tooltip("Sync probe: seed, set_data, rollback. Data restored to original.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_optimistic_rollback(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-optimistic-flow")
                    .outline()
                    .label("Full mutation")
                    .disabled(is_sending)
                    .tooltip("Sync probe: seed 'original' \u{2192} set_data('optimistic') \u{2192} complete('server confirmed'). data='server confirmed' previous='optimistic'.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_optimistic_flow(cx);
                    })),
            ),
        )
        .child(
            div().px_4().pb_3().child(
                toggle_button(
                    "http-lab-testing-toggle-optimistic-details",
                    "Optimistic details",
                    self.show_optimistic_details,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_optimistic_details(cx))),
            ),
        )
        .when(self.show_optimistic_details, |section| {
            section.child(
                div().px_4().py_3().child(
                    v_flex().gap_2()
                        .child(query_resource_row("Optimistic resource", optimistic_resource, cx))
                        .child(row("Data", &opt_data, cx))
                        .child(row("Previous data", &opt_previous, cx))
                        .child(row("Display data", &opt_display, cx))
                        .child(row("Status", &opt_status, cx))
                        .child(row("Optimistic message", &self.query_optimistic_message, cx)),
                ),
            )
        });

        // -- Section 5: Standalone Client Fetch --
        let client_fetch_section = section_card(
            "Standalone Client Fetch",
            "Test QueryClient.fetch_query() and force_fetch_query() \u{2014} no component subscription needed",
            cx,
        )
        .child(
            div().flex().flex_wrap().gap_2().px_4().py_3()
            .child(
                Button::new("http-lab-testing-client-fetch")
                    .outline()
                    .label("Client fetch")
                    .disabled(is_sending)
                    .tooltip("Imperative fetch via QueryClient. Creates resource and starts request without a component. Check message below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_client_fetch_query(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-client-force")
                    .outline()
                    .label("Client force fetch")
                    .disabled(is_sending)
                    .tooltip("Same but bypasses cache freshness checks. Check message below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_client_force_fetch_query(cx);
                    })),
            ),
        )
        .child(
            div().px_4().pb_3().child(
                toggle_button(
                    "http-lab-testing-toggle-client-details",
                    "Client details",
                    self.show_client_details,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_client_details(cx))),
            ),
        )
        .when(self.show_client_details, |section| {
            section.child(
                div().px_4().py_3().child(
                    v_flex()
                        .gap_2()
                        .child(row("Client message", &self.client_query_message, cx)),
                ),
            )
        });

        // -- Section 6: Local Full Lab --
        let local_lab_section = section_card(
            "Local Full Lab",
            "Full integration: each action uses its own QueryResource with real cache/request policies, real HTTP calls",
            cx,
        )
        .child(
            div().flex().flex_wrap().gap_2().px_4().py_3()
            .children(HttpLabAction::all().iter().copied().map(|action| {
                let tip = match action {
                    HttpLabAction::GetText => "Sends GET to httpbingo.org/encoding/utf8 (TTL 60s, LatestWins). Populates the local lab resource panel.",
                    HttpLabAction::GetXml => "Sends GET to httpbingo.org/xml (TTL 60s, LatestWins). Populates the local lab resource panel.",
                    HttpLabAction::GetJson => "Sends GET to httpbingo.org/json (StaleWhileRevalidate 30s, LatestWins). Populates the local lab resource panel.",
                    HttpLabAction::PostJson => "Sends POST to httpbingo.org/post (NoCache, LatestWins). Populates the local lab resource panel.",
                    HttpLabAction::PostForm => "Sends POST to httpbingo.org/post (NoCache, LatestWins). Populates the local lab resource panel.",
                    HttpLabAction::PostMultipart => "Sends POST to httpbingo.org/post (NoCache, IgnoreWhileLoading). Duplicates are ignored while loading.",
                    HttpLabAction::Cookies => "Sends GET to httpbingo.org/cookies (NoCache, LatestWins). Populates the local lab resource panel.",
                    HttpLabAction::Failure => "Sends GET to httpbingo.org/status/418 (NoCache, LatestWins). Expect a 418 error response.",
                    HttpLabAction::FullFlow => "Runs 4 sequential requests (GetJson, PostJson, Cookies, Failure) and populates all individual resources plus the FullFlow resource.",
                };
                Button::new(format!("http-lab-testing-local-{}", action.id()))
                    .outline()
                    .label(format!("Local {}", action.label()))
                    .disabled(is_sending)
                    .tooltip(tip)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.send_local_lab_action(action, cx);
                    }))
            }))
            .child(
                Button::new("http-lab-testing-local-reset")
                    .outline()
                    .label("Local reset")
                    .disabled(is_sending)
                    .tooltip("Resets all local lab resources to Idle, advances the request sequencer scope, and clears history.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reset_local_lab(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-local-cancel")
                    .danger()
                    .outline()
                    .label("Cancel active")
                    .disabled(!is_sending)
                    .tooltip("Cancels any in-flight request via CancellationToken.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel(cx);
                    })),
            ),
        )
        .child(
            div().px_4().py_3().child(
                v_flex()
                    .gap_2()
                    .child(row("Selected", self.local_lab_selected.label(), cx))
                    .child(row("Message", &self.local_lab_message, cx))
                    .child(row(
                        "History",
                        &self.local_lab_history.len().to_string(),
                        cx,
                    ))
                    .child(toggle_button(
                        "http-lab-testing-toggle-local-history",
                        "History details",
                        self.show_local_history,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_local_history(cx))))
                    .children(HttpLabAction::all().iter().copied().map(|action| {
                        let resource = self
                            .local_lab_resources
                            .get(&action)
                            .expect("local lab resource must exist");
                        query_resource_row(action.label(), resource, cx)
                    }))
                    .when(self.show_local_history, |this| {
                        this.child(local_lab_history_panel(self, cx))
                    }),
            ),
        );

        // -- Section 7: Raw Baseline --
        let raw_baseline_section = section_card(
            "Raw Baseline",
            "Plain reqwest GET with manual operation tracking \u{2014} no gpui-query involved",
            cx,
        )
        .child(
            div().flex().flex_wrap().gap_2().px_4().py_3()
            .child(
                Button::new("http-lab-testing-send")
                    .outline()
                    .label(if is_sending {
                        "Sending raw GET"
                    } else {
                        "Send raw GET"
                    })
                    .disabled(is_sending)
                    .tooltip("Plain reqwest GET to httpbingo.org. No QueryResource. Baseline for comparison.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send_raw_get(cx);
                    })),
            ),
        )
        .child(
            div().px_4().py_3().child(
                v_flex()
                    .gap_2()
                    .child(row("Status", self.status.label(), cx))
                    .child(row(
                        "Active operation",
                        &self
                            .active_operation_id
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "none".to_string()),
                        cx,
                    ))
                    .child(row("Message", &self.last_message, cx))
                    .child(toggle_button(
                        "http-lab-testing-toggle-response-details",
                        "Response details",
                        self.show_response_details,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_response_details(cx))))
                    .when(self.show_response_details, |this| {
                        this.when_some(self.last_response.as_ref(), |this, response| {
                            this.child(row("Response status", &response.status.to_string(), cx))
                                .child(row("Response URL", &response.final_url, cx))
                                .child(row(
                                    "Response headers",
                                    &response.header_count.to_string(),
                                    cx,
                                ))
                                .child(row("Preview bytes", &response.bytes.to_string(), cx))
                                .child(toggle_button(
                                    "http-lab-testing-toggle-response-preview",
                                    "Response preview",
                                    self.show_response_preview,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_response_preview(cx))))
                                .when(self.show_response_preview, |this| {
                                    this.child(
                                        div()
                                            .p_3()
                                            .rounded(cx.theme().radius)
                                            .bg(cx.theme().muted)
                                            .text_xs()
                                            .font_family("monospace")
                                            .child(preview_excerpt(&response.preview, 1024)),
                                    )
                                })
                        })
                    })
                    .when(self.last_response.is_none(), |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("No response captured."),
                        )
                    }),
            ),
        );

        let view = v_flex()
            .min_h_full()
            .p_6()
            .gap_5()
            .child(
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
                                    .child("HTTP Lab Testing"),
                            )
                            .child(
                                div()
                                    .max_w(px(760.))
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Raw reqwest-only screen for isolating GPUI task scheduling from the existing HTTP Lab store and gpui-query path."),
                            ),
                    ),
            )
            .child(query_lifecycle_section)
            .child(cancel_signal_section)
            .child(data_retention_section)
            .child(optimistic_section)
            .child(client_fetch_section)
            .child(local_lab_section)
            .child(raw_baseline_section);

        tracing::debug!(
            target: RENDER_LOG,
            elapsed_us = render_started.elapsed().as_micros() as u64,
            status = self.status.label(),
            is_sending,
            active_operation_id,
            query_status = query_status.label(),
            local_selected = local_selected.id(),
            local_history_len,
            "HTTP Lab Testing render completed"
        );

        view
    }
}
