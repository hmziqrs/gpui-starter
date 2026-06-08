use gpui::{prelude::*, *};
use gpui_component::{
    Disableable as _, button::{Button, ButtonVariants as _}, v_flex,
};


use super::super::{
    ui_helpers::{
        compact_resource_preview, query_resource_row, row, section_card, toggle_button,
    },
    HttpLabTestingPage, RawStatus,
};

impl HttpLabTestingPage {
    /// Section 1: Query Lifecycle
    pub(super) fn render_query_lifecycle_section(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_sending = matches!(self.status, RawStatus::Sending);

        section_card(
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
        })
    }

    /// Section 2: Cancel Signal
    pub(super) fn render_cancel_signal_section(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_sending = matches!(self.status, RawStatus::Sending);
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

        section_card(
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
        })
    }

    /// Section 3: Cache & Data Retention
    pub(super) fn render_data_retention_section(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_sending = matches!(self.status, RawStatus::Sending);
        let placeholder_resource = &self.query_placeholder_resource;
        let ph_data = compact_resource_preview(placeholder_resource.data());
        let ph_placeholder = compact_resource_preview(placeholder_resource.placeholder_data());
        let ph_display = compact_resource_preview(placeholder_resource.display_data());
        let ph_previous = compact_resource_preview(placeholder_resource.previous_data());

        section_card(
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
        })
    }
}
