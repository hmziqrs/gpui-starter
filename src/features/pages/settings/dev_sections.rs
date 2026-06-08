use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, WindowExt as _,
    button::Button,
    label::Label,
    switch::Switch,
    v_flex,
};

use crate::connectivity;
use crate::desktop_actions;
use crate::secure_storage;
use crate::session::{self, SessionState};
use crate::telemetry::{self, TelemetryMode};

/// Renders the "Shortcuts" settings card.
pub(super) fn render_shortcuts_section(
    app_config: &crate::app_state::AppConfig,
    cx: &mut Context<super::SettingsPage>,
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
pub(super) fn render_storage_section(
    cx: &mut Context<super::SettingsPage>,
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
pub(super) fn render_developer_section(
    app_config: &crate::app_state::AppConfig,
    cx: &mut Context<super::SettingsPage>,
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

/// Renders the "Desktop Actions" settings card.
pub(super) fn render_desktop_actions_section(
    cx: &mut Context<super::SettingsPage>,
) -> impl IntoElement {
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
        )
}

/// Renders the "Telemetry" mode selection card.
pub(super) fn render_telemetry_section(
    cx: &mut Context<super::SettingsPage>,
) -> impl IntoElement {
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
        )
}

/// Renders the "Telemetry Runtime" card (record event, error, user property, flush).
pub(super) fn render_telemetry_runtime_section(
    cx: &mut Context<super::SettingsPage>,
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

/// Renders the "Runtime Boundaries" card (connectivity, session, secure storage).
pub(super) fn render_runtime_boundaries_section(
    cx: &mut Context<super::SettingsPage>,
) -> impl IntoElement {
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
        )
}

/// Renders the "Event Emitter" card (emit buttons + receiver log).
pub(super) fn render_event_emitter_section(
    event_log: &[String],
    cx: &mut Context<super::SettingsPage>,
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
                        .label("Emit Navigate \u{2192} Home")
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
