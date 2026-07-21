use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Selectable as _,
    button::{Button, ButtonVariants as _},
    label::Label,
    switch::Switch,
    v_flex,
};

pub struct HomePage;

impl HomePage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HomePage {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for HomePage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let first_run_pending = crate::first_run::is_pending(cx);
        let locale = crate::app::current_locale(cx);
        let notifications_enabled = crate::notifications::snapshot(cx).enabled_by_user;

        v_flex()
            .min_h_full()
            .items_center()
            .justify_center()
            .gap_6()
            .child(
                div()
                    .text_3xl()
                    .font_weight(FontWeight::BOLD)
                    .child(crate::i18n::localize("home_title", None)),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(crate::i18n::localize("home_subtitle", None)),
            )
            .child(
                Button::new("get-started")
                    .primary()
                    .label(crate::i18n::localize("home_get_started", None))
                    .on_click(|_, _, _| {
                        tracing::info!("Get Started clicked");
                    }),
            )
            .child(
                Button::new("start-demo-task")
                    .outline()
                    .label("Start Demo Task")
                    .on_click(cx.listener(|_, _, window, cx| {
                        tracing::info!(
                            target: "gpui_starter::features::pages::home",
                            active_tasks_before = crate::tasks::active_count(cx),
                            "Start Demo Task clicked"
                        );
                        crate::tasks::start_demo_task_in_window(window, cx);
                    })),
            )
            .when(first_run_pending, |this| {
                this.child(
                    v_flex()
                        .w(px(520.))
                        .gap_3()
                        .p_4()
                        .rounded_lg()
                        .border_1()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::BOLD)
                                .child("First-run setup"),
                        )
                        .child(Label::new(
                            "Choose defaults now. You can change these later in Settings.",
                        ))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(Label::new("Locale"))
                                .child(
                                    div()
                                        .flex()
                                        .gap_2()
                                        .child(
                                            Button::new("first-run-locale-en")
                                                .outline()
                                                .selected(locale.as_ref() == crate::app::LOCALE_EN)
                                                .label("English")
                                                .on_click(|_, _, cx| {
                                                    crate::app::set_locale(
                                                        crate::app::LOCALE_EN,
                                                        cx,
                                                    );
                                                }),
                                        )
                                        .child(
                                            Button::new("first-run-locale-zh-cn")
                                                .outline()
                                                .selected(
                                                    locale.as_ref() == crate::app::LOCALE_ZH_CN,
                                                )
                                                .label("简体中文")
                                                .on_click(|_, _, cx| {
                                                    crate::app::set_locale(
                                                        crate::app::LOCALE_ZH_CN,
                                                        cx,
                                                    );
                                                }),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(Label::new("Native notifications"))
                                .child(
                                    Switch::new("first-run-notifications")
                                        .checked(notifications_enabled)
                                        .on_click(|checked, _, cx| {
                                            crate::notifications::set_native_notifications_enabled(
                                                *checked, cx,
                                            );
                                        }),
                                ),
                        )
                        .child(
                            div().flex().items_center().gap_2().child(
                                Button::new("first-run-complete")
                                    .primary()
                                    .label("Finish setup")
                                    .on_click(|_, _, cx| {
                                        crate::first_run::complete(cx);
                                    }),
                            ),
                        ),
                )
            })
    }
}
