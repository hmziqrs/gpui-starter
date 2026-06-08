use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _,
    button::Button,
    label::Label,
    switch::Switch,
    v_flex,
};

/// Renders the "Shortcuts" settings card.
pub fn render_shortcuts_section(
    app_config: &crate::app_state::AppConfig,
    cx: &mut Context<super::super::SettingsPage>,
) -> impl IntoElement {
    let app_config = app_config.clone();
    v_flex()
        .gap_3()
        .p_4()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(Label::new("Shortcuts"))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(Label::new("Enable global launcher shortcut (macOS)"))
                .child(
                    Switch::new("global-shortcut-enabled")
                        .checked(app_config.global_shortcut_enabled)
                        .on_click(|checked, _, cx| {
                            crate::app_state::update_config(cx, |config| {
                                config.global_shortcut_enabled = *checked;
                            });
                            crate::shortcuts::apply_enabled(*checked, cx);
                        }),
                ),
        )
}

/// Renders the "Storage" settings card.
pub fn render_storage_section(
    cx: &mut Context<super::super::SettingsPage>,
) -> impl IntoElement {
    v_flex()
        .gap_3()
        .p_4()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(Label::new("Storage"))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("storage-health-check")
                        .outline()
                        .label("Run Health Check")
                        .on_click(|_, _, cx| {
                            crate::storage::run_health_check(cx);
                        }),
                )
                .child(
                    Button::new("storage-maintenance")
                        .outline()
                        .label("Run Maintenance")
                        .on_click(|_, _, cx| {
                            crate::storage::run_maintenance(cx);
                        }),
                ),
        )
}

/// Renders the "Developer" settings card (frame-time toggle).
pub fn render_developer_section(
    app_config: &crate::app_state::AppConfig,
    cx: &mut Context<super::super::SettingsPage>,
) -> impl IntoElement {
    let app_config = app_config.clone();
    v_flex()
        .gap_3()
        .p_4()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(Label::new("Developer"))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(Label::new("Show Frame Time"))
                .child(
                    Switch::new("show-frame-time")
                        .checked(app_config.show_frame_time)
                        .on_click(|checked, _, cx| {
                            crate::app_state::update_config(cx, |config| {
                                config.show_frame_time = *checked;
                            });
                        }),
                ),
        )
}
