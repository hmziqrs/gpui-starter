mod query_sections;
mod mutation_sections;
mod lab_sections;

use std::time::Instant;

use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme as _, v_flex};

use super::{HttpLabTestingPage, RawStatus, RENDER_LOG};

impl Render for HttpLabTestingPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = Instant::now();
        let is_sending = matches!(self.status, RawStatus::Sending);
        let active_operation_id = self.active_operation_id;
        let query_status = self.query_resource.status();
        let local_selected = self.local_lab_selected;
        let local_history_len = self.local_lab_history.len();

        // Extract theme colors before render section calls to avoid overlapping
        // mutable borrows of `cx` (Rust 2024 impl Trait lifetime capture).
        let radius_lg = cx.theme().radius_lg;
        let border = cx.theme().border;
        let muted = cx.theme().muted;
        let muted_foreground = cx.theme().muted_foreground;

        let view = v_flex()
            .min_h_full()
            .p_6()
            .gap_5()
            .child(
                div()
                    .p_5()
                    .rounded(radius_lg)
                    .border_1()
                    .border_color(border)
                    .bg(muted)
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
                                    .text_color(muted_foreground)
                                    .child("Raw reqwest-only screen for isolating GPUI task scheduling from the existing HTTP Lab store and gpui-query path."),
                            ),
                    ),
            )
            .child(self.render_query_lifecycle_section(cx))
            .child(self.render_cancel_signal_section(cx))
            .child(self.render_data_retention_section(cx))
            .child(self.render_optimistic_section(cx))
            .child(self.render_client_fetch_section(cx))
            .child(self.render_local_lab_section(cx))
            .child(self.render_raw_baseline_section(cx));

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
