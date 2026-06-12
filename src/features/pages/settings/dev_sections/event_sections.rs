use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme as _, button::Button, label::Label, v_flex};

/// Renders the "Event Emitter" card (emit buttons + receiver log).
pub fn render_event_emitter_section(
    event_log: &[String],
    cx: &mut Context<super::super::SettingsPage>,
) -> impl IntoElement {
    v_flex()
        .gap_3()
        .p_4()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(Label::new("Event Emitter"))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Test the event pipeline. Emit events and verify they are received."),
        )
        // Emit buttons
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    Button::new("emit-test-noop")
                        .outline()
                        .label("Emit Test (No-op)")
                        .on_click(|_, _, cx| {
                            crate::events::emit(
                                crate::events::AppEventKind::Test {
                                    message: "hello from settings".into(),
                                },
                                cx,
                            );
                        }),
                )
                .child(
                    Button::new("emit-navigate-home")
                        .outline()
                        .label("Emit Navigate \u{2192} Home")
                        .on_click(|_, _, cx| {
                            crate::events::emit(
                                crate::events::AppEventKind::Navigate(
                                    crate::routes::AppRoute::page(crate::sidebar::Page::Home),
                                ),
                                cx,
                            );
                        }),
                )
                .child(
                    Button::new("emit-navigate-notifications")
                        .outline()
                        .label("Emit Navigate \u{2192} Notifications")
                        .on_click(|_, _, cx| {
                            crate::events::emit(
                                crate::events::AppEventKind::Navigate(
                                    crate::routes::AppRoute::page(
                                        crate::sidebar::Page::Notifications,
                                    ),
                                ),
                                cx,
                            );
                        }),
                ),
        )
        // Receiver log
        .child(Label::new("Event Receiver"))
        .child(
            v_flex()
                .gap_1()
                .when(event_log.is_empty(), |el| {
                    el.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("No events received yet. Click a button above."),
                    )
                })
                .children(event_log.iter().rev().map(|entry| {
                    div()
                        .text_xs()
                        .p_1()
                        .rounded(px(4.))
                        .bg(cx.theme().muted)
                        .child(entry.clone())
                })),
        )
}
