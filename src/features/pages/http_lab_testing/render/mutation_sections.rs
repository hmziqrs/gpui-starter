use gpui::{prelude::*, *};
use gpui_component::{
    Disableable as _, button::Button, v_flex,
};

use super::super::{
    ui_helpers::{compact_resource_preview, query_resource_row, row, section_card, toggle_button},
    HttpLabTestingPage, RawStatus,
};

impl HttpLabTestingPage {
    /// Section 4: Optimistic Updates
    pub(super) fn render_optimistic_section(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_sending = matches!(self.status, RawStatus::Sending);
        let optimistic_resource = &self.query_optimistic_resource;
        let opt_data = compact_resource_preview(optimistic_resource.data());
        let opt_previous = compact_resource_preview(optimistic_resource.previous_data());
        let opt_display = compact_resource_preview(optimistic_resource.display_data());
        let opt_status = optimistic_resource.status().label().to_string();

        section_card(
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
        })
    }

    /// Section 5: Standalone Client Fetch
    pub(super) fn render_client_fetch_section(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_sending = matches!(self.status, RawStatus::Sending);

        section_card(
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
        })
    }
}
