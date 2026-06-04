use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Theme, WindowExt as _,
    button::{Button, ButtonVariants as _},
    label::Label,
    switch::Switch,
    v_flex,
};

use crate::app::{self, LOCALE_EN, LOCALE_ZH_CN, LocaleState};
use crate::app_state;
use crate::connectivity;
use crate::desktop_actions;
use crate::notifications::{
    self, NativeNotificationState, NotificationPermissionState, NotificationRequest,
    NotificationRuntimeSnapshot,
};
use crate::secure_storage;
use crate::session::{self, SessionState};
use crate::telemetry::{self, TelemetryMode};

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
        let can_request_permission = notifications_snapshot.capabilities.can_request_permission
            && matches!(
                notifications_snapshot.permission,
                NotificationPermissionState::NotDetermined
                    | NotificationPermissionState::Unknown
                    | NotificationPermissionState::Unavailable(_)
            );
        let can_open_settings = cfg!(target_os = "macos")
            && matches!(
                notifications_snapshot.permission,
                NotificationPermissionState::Denied | NotificationPermissionState::Unavailable(_)
            );

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
            .child(
                v_flex()
                    .gap_3()
                    .p_4()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(Label::new(crate::i18n::localize(
                                "settings_native_notifications",
                                None,
                            )))
                            .child(
                                Switch::new("native-notifications-enabled")
                                    .checked(notifications_snapshot.enabled_by_user)
                                    .on_click(|checked, _, cx| {
                                        notifications::set_native_notifications_enabled(
                                            *checked, cx,
                                        );
                                    }),
                            ),
                    )
                    .child(status_row(
                        crate::i18n::localize("settings_native_backend", None),
                        notifications_snapshot.active_backend.to_string(),
                    ))
                    .child(status_row(
                        crate::i18n::localize("settings_permission", None),
                        notifications_snapshot.permission.label(),
                    ))
                    .when_some(
                        notifications_snapshot.degraded_reason.clone(),
                        |this, reason| {
                            this.child(status_row(
                                crate::i18n::localize("settings_degraded", None),
                                reason,
                            ))
                        },
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("test-native-notification")
                                    .primary()
                                    .label(crate::i18n::localize(
                                        "settings_test_native_notification",
                                        None,
                                    ))
                                    .on_click(|_, window, cx| {
                                        notifications::send_from_window(
                                            NotificationRequest::foreground(
                                                crate::i18n::localize(
                                                    "settings_test_native_notification",
                                                    None,
                                                ),
                                                crate::i18n::localize(
                                                    "settings_hello_notification",
                                                    None,
                                                ),
                                            ),
                                            window,
                                            cx,
                                        );
                                    }),
                            )
                            .child(
                                Button::new("request-notification-permission")
                                    .outline()
                                    .disabled(!can_request_permission)
                                    .label(crate::i18n::localize(
                                        "settings_request_permission",
                                        None,
                                    ))
                                    .on_click(|_, window, cx| {
                                        notifications::request_permission_from_window(window, cx);
                                    }),
                            )
                            .child(
                                Button::new("open-notification-settings")
                                    .outline()
                                    .disabled(!can_open_settings)
                                    .label(crate::i18n::localize(
                                        "settings_open_notification_settings",
                                        None,
                                    ))
                                    .on_click(|_, _, cx| {
                                        notifications::open_system_settings(cx);
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("test-action-notification")
                                    .outline()
                                    .label(crate::i18n::localize(
                                        "settings_test_action_notification",
                                        None,
                                    ))
                                    .on_click(|_, window, cx| {
                                        notifications::send_from_window(
                                            NotificationRequest::action_buttons(
                                                crate::i18n::localize(
                                                    "settings_test_action_notification",
                                                    None,
                                                ),
                                                crate::i18n::localize(
                                                    "settings_action_notification_body",
                                                    None,
                                                ),
                                            ),
                                            window,
                                            cx,
                                        );
                                    }),
                            )
                            .child(
                                Button::new("test-reply-notification")
                                    .outline()
                                    .label(crate::i18n::localize(
                                        "settings_test_reply_notification",
                                        None,
                                    ))
                                    .on_click(|_, window, cx| {
                                        notifications::send_from_window(
                                            NotificationRequest::reply(
                                                crate::i18n::localize(
                                                    "settings_test_reply_notification",
                                                    None,
                                                ),
                                                crate::i18n::localize(
                                                    "settings_reply_notification_body",
                                                    None,
                                                ),
                                            ),
                                            window,
                                            cx,
                                        );
                                    }),
                            )
                            .child(
                                Button::new("test-background-worthy-notification")
                                    .outline()
                                    .label(crate::i18n::localize(
                                        "settings_test_background_notification",
                                        None,
                                    ))
                                    .on_click(|_, window, cx| {
                                        notifications::send_from_window(
                                            NotificationRequest::background_worthy(
                                                crate::i18n::localize(
                                                    "settings_test_background_notification",
                                                    None,
                                                ),
                                                crate::i18n::localize(
                                                    "settings_background_notification_body",
                                                    None,
                                                ),
                                            ),
                                            window,
                                            cx,
                                        );
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(crate::i18n::localize(
                                "settings_in_app_notifications_note",
                                None,
                            )),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(crate::i18n::localize(
                                "settings_push_notifications_note",
                                None,
                            )),
                    ),
            )
            .child(
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
                    ),
            )
            .child(
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
                    ),
            )
            .child(
                v_flex()
                    .gap_3()
                    .p_4()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(Label::new("Desktop Actions"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("desktop-copy-diagnostics")
                                    .outline()
                                    .label("Copy Diagnostics")
                                    .on_click(|_, _, cx| {
                                        let _ = desktop_actions::copy_diagnostics(cx);
                                    }),
                            )
                            .child(
                                Button::new("desktop-open-logs")
                                    .outline()
                                    .label("Open Logs Folder")
                                    .on_click(|_, _, cx| {
                                        let _ = desktop_actions::open_logs_folder(cx);
                                    }),
                            )
                            .child(
                                Button::new("desktop-open-config")
                                    .outline()
                                    .label("Open Config Folder")
                                    .on_click(|_, _, cx| {
                                        let _ = desktop_actions::open_config_folder(cx);
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("desktop-pick-file")
                                    .outline()
                                    .label("Pick File")
                                    .on_click(|_, _, cx| {
                                        let _ = desktop_actions::pick_file(cx);
                                    }),
                            )
                            .child(
                                Button::new("desktop-pick-folder")
                                    .outline()
                                    .label("Pick Folder")
                                    .on_click(|_, _, cx| {
                                        let _ = desktop_actions::pick_folder(cx);
                                    }),
                            ),
                    )
                    .child(
                        div().flex().items_center().gap_2().child(
                            Button::new("desktop-save-file")
                                .outline()
                                .label("Save File")
                                .on_click(|_, _, cx| {
                                    let _ = desktop_actions::save_file(cx);
                                }),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("desktop-watch-logs")
                                    .outline()
                                    .label("Watch Logs Dir")
                                    .on_click(|_, _, cx| {
                                        let _ = desktop_actions::watch_log_dir(cx);
                                    }),
                            )
                            .child(
                                Button::new("desktop-watch-config")
                                    .outline()
                                    .label("Watch Config Dir")
                                    .on_click(|_, _, cx| {
                                        let _ = desktop_actions::watch_config_dir(cx);
                                    }),
                            )
                            .child(
                                Button::new("desktop-unwatch-all")
                                    .outline()
                                    .label("Unwatch All")
                                    .on_click(|_, _, cx| {
                                        let _ = desktop_actions::unwatch_all(cx);
                                    }),
                            )
                            .child(
                                Button::new("desktop-open-support-url")
                                    .outline()
                                    .label("Open Support URL")
                                    .on_click(|_, _, cx| {
                                        let _ = desktop_actions::open_url(
                                            "https://example.com/support",
                                            cx,
                                        );
                                    }),
                            ),
                    ),
            )
            .child(
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
                            .child(
                                "Telemetry export is disabled by default until explicit consent.",
                            ),
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
                                        telemetry::set_mode(
                                            TelemetryMode::Disabled,
                                            false,
                                            None,
                                            cx,
                                        );
                                    }),
                            )
                            .child(
                                Button::new("telemetry-local")
                                    .outline()
                                    .label("Local Only")
                                    .on_click(|_, _, cx| {
                                        telemetry::set_mode(
                                            TelemetryMode::LocalOnly,
                                            true,
                                            None,
                                            cx,
                                        );
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
                    ),
            )
            .child(
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
                    ),
            )
            // Connectivity + Session + Secure storage dev controls
            .child(
                v_flex()
                    .gap_3()
                    .p_4()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(Label::new("Runtime Boundaries"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("connectivity-check-now")
                                    .outline()
                                    .label("Check Connectivity Now")
                                    .on_click(|_, _, cx| {
                                        connectivity::check_now(cx);
                                    }),
                            )
                            .child(
                                Button::new("session-sign-in")
                                    .outline()
                                    .label("Session Sign In (Demo)")
                                    .on_click(|_, _, cx| {
                                        session::set_state(SessionState::SigningIn, cx);
                                        session::set_state(
                                            SessionState::SignedIn {
                                                account_label: "demo-user".to_string(),
                                            },
                                            cx,
                                        );
                                    }),
                            )
                            .child(
                                Button::new("session-sign-out")
                                    .outline()
                                    .label("Session Sign Out")
                                    .on_click(|_, _, cx| {
                                        session::set_state(SessionState::SignedOut, cx);
                                    }),
                            )
                            .child(
                                Button::new("session-error-demo")
                                    .outline()
                                    .label("Session Error (Demo)")
                                    .on_click(|_, _, cx| {
                                        session::set_state(
                                            SessionState::Error("demo session error".to_string()),
                                            cx,
                                        );
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("secure-storage-write-demo")
                                    .outline()
                                    .label("Write Secure Value (Demo)")
                                    .on_click(|_, window, cx| {
                                        let message = match secure_storage::set_secret(
                                            "gpui-starter",
                                            "demo-token",
                                            "demo-value",
                                            cx,
                                        ) {
                                            Ok(()) => "Secure value written".to_string(),
                                            Err(err) => format!("Write failed: {err}"),
                                        };
                                        window.push_notification(message, cx);
                                    }),
                            )
                            .child(
                                Button::new("secure-storage-delete-demo")
                                    .outline()
                                    .label("Delete Secure Value (Demo)")
                                    .on_click(|_, window, cx| {
                                        let message = match secure_storage::delete_secret(
                                            "gpui-starter",
                                            "demo-token",
                                            cx,
                                        ) {
                                            Ok(()) => "Secure value deleted".to_string(),
                                            Err(err) => format!("Delete failed: {err}"),
                                        };
                                        window.push_notification(message, cx);
                                    }),
                            ),
                    )
                    .child(
                        Button::new("secure-storage-read-demo")
                            .outline()
                            .label("Read Secure Value (Demo)")
                            .on_click(|_, window, cx| {
                                let message = match secure_storage::get_secret(
                                    "gpui-starter",
                                    "demo-token",
                                    cx,
                                ) {
                                    Ok(Some(_)) => "Secure value exists".to_string(),
                                    Ok(None) => "Secure value missing".to_string(),
                                    Err(err) => format!("Secure storage read failed: {err}"),
                                };
                                window.push_notification(message, cx);
                            }),
                    ),
            )
            // -- Event Emitter --
            .child(
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
                        div().flex().flex_wrap().items_center().gap_2()
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
                                    .label("Emit Navigate → Home")
                                    .on_click(|_, _, cx| {
                                        crate::events::emit(
                                            crate::events::AppEventKind::Navigate(
                                                crate::routes::AppRoute::page(
                                                    crate::sidebar::Page::Home,
                                                ),
                                            ),
                                            cx,
                                        );
                                    }),
                            )
                            .child(
                                Button::new("emit-navigate-notifications")
                                    .outline()
                                    .label("Emit Navigate → Notifications")
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
                            .when(self.event_log.is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("No events received yet. Click a button above."),
                                )
                            })
                            .children(self.event_log.iter().rev().map(|entry| {
                                div()
                                    .text_xs()
                                    .p_1()
                                    .rounded(px(4.))
                                    .bg(cx.theme().muted)
                                    .child(entry.clone())
                            })),
                    ),
            )
    }
}

fn status_row(label: impl Into<SharedString>, value: impl Into<SharedString>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(Label::new(label.into()))
        .child(div().text_sm().child(value.into()))
}
