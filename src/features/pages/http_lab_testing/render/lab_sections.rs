use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Disableable as _,
    button::{Button, ButtonVariants as _},
    v_flex,
};

use crate::services::http_lab::HttpLabAction;

use super::super::{
    HttpLabTestingPage, RawStatus,
    ui_helpers::{
        local_lab_history_panel, preview_excerpt, query_resource_row, row, section_card,
        toggle_button,
    },
};

impl HttpLabTestingPage {
    /// Section 6: Local Full Lab
    pub(super) fn render_local_lab_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_sending = matches!(self.status, RawStatus::Sending);

        section_card(
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
        )
    }

    /// Section 7: Raw Baseline
    pub(super) fn render_raw_baseline_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_sending = matches!(self.status, RawStatus::Sending);

        section_card(
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
        )
    }
}
