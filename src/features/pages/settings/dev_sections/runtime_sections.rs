use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, WindowExt as _,
    button::Button,
    label::Label,
    v_flex,
};

use crate::connectivity;
use crate::desktop_actions;
use crate::secure_storage;
use crate::session::{self, SessionState};

/// Renders the "Desktop Actions" settings card.
pub fn render_desktop_actions_section(
    cx: &mut Context<super::super::SettingsPage>,
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

/// Renders the "Runtime Boundaries" card (connectivity, session, secure storage).
pub fn render_runtime_boundaries_section(
    cx: &mut Context<super::super::SettingsPage>,
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
