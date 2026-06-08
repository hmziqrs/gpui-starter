mod dev_sections;
mod notifications_section;

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Selectable as _, Theme, WindowExt as _,
    button::Button,
    label::Label,
    switch::Switch,
    v_flex,
};

use crate::app::{self, LOCALE_EN, LOCALE_ZH_CN, LocaleState};
use crate::app_state;
use crate::notifications::{self, NativeNotificationState, NotificationRuntimeSnapshot};

pub struct SettingsPage {
    dark_mode: bool,
    locale: SharedString,
    notifications: NotificationRuntimeSnapshot,
    /// Log of received event descriptions for the Event Emitter test section.
    event_log: Vec<String>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _subscriptions = vec![
            cx.observe_global_in::<Theme>(window, |this, _, cx| {
                let dark_mode = cx.theme().mode.is_dark();
                if this.dark_mode != dark_mode {
                    this.dark_mode = dark_mode;
                    cx.notify();
                }
            }),
            cx.observe_global_in::<LocaleState>(window, |this, _, cx| {
                let locale = app::current_locale(cx);
                if this.locale != locale {
                    this.locale = locale;
                    cx.notify();
                }
            }),
            cx.observe_global_in::<NativeNotificationState>(window, |this, _, cx| {
                this.notifications = notifications::snapshot(cx);
                cx.notify();
            }),
            cx.observe_global_in::<crate::events::AppEventQueue>(window, |this, _, cx| {
                // Peek at events without draining — AppRoot owns the drain.
                if let Some(queue) = cx.try_global::<crate::events::AppEventQueue>() {
                    for event in &queue.0 {
                        let desc = format!("{:?} ({})", event.kind, event.id);
                        this.event_log.push(desc);
                    }
                }
                // Keep only last 20 entries
                if this.event_log.len() > 20 {
                    let drain = this.event_log.len() - 20;
                    this.event_log.drain(0..drain);
                }
                cx.notify();
            }),
        ];

        Self {
            dark_mode: cx.theme().mode.is_dark(),
            locale: app::current_locale(cx),
            notifications: notifications::snapshot(cx),
            event_log: Vec::new(),
            _subscriptions,
        }
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.locale.clone();
        let is_dark = self.dark_mode;
        let notifications_snapshot = self.notifications.clone();
        let app_config = app_state::config(cx);

        v_flex()
            .min_h_full()
            .p_6()
            .gap_6()
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .child(crate::i18n::localize("settings_title", None)),
            )
            // Dark mode toggle
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(Label::new(crate::i18n::localize(
                        "settings_dark_mode",
                        None,
                    )))
                    .child(Switch::new("dark-mode").checked(is_dark).on_click(
                        move |checked, _, cx| {
                            let mode = if *checked {
                                gpui_component::ThemeMode::Dark
                            } else {
                                gpui_component::ThemeMode::Light
                            };
                            app::set_theme_mode(mode, cx);
                        },
                    )),
            )
            // Language selection
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(Label::new(crate::i18n::localize("settings_language", None)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("settings-language-en")
                                    .outline()
                                    .selected(locale.as_ref() == LOCALE_EN)
                                    .label(crate::i18n::localize("settings_language_english", None))
                                    .on_click(|_, _, cx| {
                                        app::set_locale(LOCALE_EN, cx);
                                    }),
                            )
                            .child(
                                Button::new("settings-language-zh-cn")
                                    .outline()
                                    .selected(locale.as_ref() == LOCALE_ZH_CN)
                                    .label(crate::i18n::localize(
                                        "settings_language_simplified_chinese",
                                        None,
                                    ))
                                    .on_click(|_, _, cx| {
                                        app::set_locale(LOCALE_ZH_CN, cx);
                                    }),
                            ),
                    ),
            )
            // Native local notifications
            .child(notifications_section::render_notifications_section(
                &notifications_snapshot,
                cx,
            ))
            // Shortcuts
            .child(dev_sections::render_shortcuts_section(&app_config, cx))
            // Storage
            .child(dev_sections::render_storage_section(cx))
            // Developer
            .child(dev_sections::render_developer_section(&app_config, cx))
            // Desktop Actions
            .child(dev_sections::render_desktop_actions_section(cx))
            // Telemetry
            .child(dev_sections::render_telemetry_section(cx))
            // Telemetry Runtime
            .child(dev_sections::render_telemetry_runtime_section(cx))
            // Connectivity + Session + Secure storage dev controls
            .child(dev_sections::render_runtime_boundaries_section(cx))
            // -- Event Emitter --
            .child(dev_sections::render_event_emitter_section(
                &self.event_log,
                cx,
            ))
    }
}
