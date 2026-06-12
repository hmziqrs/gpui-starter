use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme as _, button::Button, label::Label, v_flex};

use crate::telemetry::{self, TelemetryMode};

/// Renders the "Telemetry" mode selection card.
pub fn render_telemetry_section(cx: &mut Context<super::super::SettingsPage>) -> impl IntoElement {
    v_flex()
        .gap_3()
        .p_4()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(Label::new("Telemetry"))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Telemetry export is disabled by default until explicit consent."),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("telemetry-disable")
                        .outline()
                        .label("Disable")
                        .on_click(|_, _, cx| {
                            telemetry::set_mode(TelemetryMode::Disabled, false, None, cx);
                        }),
                )
                .child(
                    Button::new("telemetry-local")
                        .outline()
                        .label("Local Only")
                        .on_click(|_, _, cx| {
                            telemetry::set_mode(TelemetryMode::LocalOnly, true, None, cx);
                        }),
                )
                .child(
                    Button::new("telemetry-remote")
                        .outline()
                        .label("Remote")
                        .on_click(|_, _, cx| {
                            telemetry::set_mode(
                                TelemetryMode::Remote,
                                true,
                                Some("https://telemetry.example.com/v1/events"),
                                cx,
                            );
                        }),
                ),
        )
}

/// Renders the "Telemetry Runtime" card (record event, error, user property, flush).
pub fn render_telemetry_runtime_section(
    cx: &mut Context<super::super::SettingsPage>,
) -> impl IntoElement {
    v_flex()
        .gap_3()
        .p_4()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(Label::new("Telemetry Runtime"))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("telemetry-record-event")
                        .outline()
                        .label("Record Test Event")
                        .on_click(|_, _, cx| {
                            telemetry::record_event("settings_test_event", cx);
                        }),
                )
                .child(
                    Button::new("telemetry-record-error")
                        .outline()
                        .label("Record Test Error")
                        .on_click(|_, _, cx| {
                            telemetry::record_error("settings_test_error", cx);
                        }),
                )
                .child(
                    Button::new("telemetry-set-user-property")
                        .outline()
                        .label("Set Test User Property")
                        .on_click(|_, _, cx| {
                            telemetry::set_user_property("plan_phase", "phase21", cx);
                        }),
                )
                .child(
                    Button::new("telemetry-flush")
                        .outline()
                        .label("Flush Telemetry")
                        .on_click(|_, _, cx| {
                            telemetry::flush(cx);
                        }),
                ),
        )
}
