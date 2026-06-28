use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Disableable as _,
    button::{Button, ButtonVariants as _},
    label::Label,
    switch::Switch,
    v_flex,
};

use crate::notifications::{
    self, NotificationPermissionState, NotificationRequest, NotificationRuntimeSnapshot,
};

/// Status label/value row used by the notifications card.
pub(super) fn status_row(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(Label::new(label.into()))
        .child(div().text_sm().child(value.into()))
}

/// Renders the "Native Local Notifications" settings card.
pub(super) fn render_notifications_section(
    notifications_snapshot: &NotificationRuntimeSnapshot,
    cx: &mut Context<super::SettingsPage>,
) -> impl IntoElement {
    let can_request_permission = notifications_snapshot.capabilities.can_request_permission
        && matches!(
            notifications_snapshot.permission,
            NotificationPermissionState::NotDetermined
                | NotificationPermissionState::Unknown
                | NotificationPermissionState::Unavailable(_)
        );
    // Linux has no per-app permission model (always "Unsupported"), so the
    // button is the user's route to GNOME Settings -> Notifications to check
    // Do-Not-Disturb / per-app banners. On macOS it only makes sense once the
    // permission is actually denied/unavailable.
    let can_open_settings = cfg!(target_os = "linux")
        || (cfg!(target_os = "macos")
            && matches!(
                notifications_snapshot.permission,
                NotificationPermissionState::Denied | NotificationPermissionState::Unavailable(_)
            ));

    let notifications_snapshot = notifications_snapshot.clone();

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
                            notifications::set_native_notifications_enabled(*checked, cx);
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
        .when_some(
            notifications_snapshot.last_backend_error.clone(),
            |this, error| this.child(status_row("Last backend error", error)),
        )
        .when_some(
            notifications_snapshot.daemon_capabilities.clone(),
            |this, caps| this.child(status_row("Daemon capabilities", caps)),
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
                                NotificationRequest::test_notification(
                                    crate::i18n::localize(
                                        "settings_test_native_notification",
                                        None,
                                    ),
                                    crate::i18n::localize("settings_hello_notification", None),
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
                        .label(crate::i18n::localize("settings_request_permission", None))
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
                                    crate::i18n::localize("settings_test_reply_notification", None),
                                    crate::i18n::localize("settings_reply_notification_body", None),
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
        )
}
