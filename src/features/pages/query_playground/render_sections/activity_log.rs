use gpui::prelude::*;
use gpui::*;

use gpui_component::{ActiveTheme as _, button::Button, h_flex};

use crate::ui::widgets::{render_virtual_list, uniform_item_sizes};

use super::super::QueryPlaygroundPage;
use super::super::ui_helpers::section_card;

impl QueryPlaygroundPage {
    pub(in super::super) fn render_activity_log(&self, cx: &mut Context<Self>) -> Div {
        let has_logs = !self.activity_log.is_empty();
        let log_count = self.activity_log.len();
        let scroll_handle = self.log_scroll_handle.clone();

        let card = section_card(
            "Activity Log",
            "Tracks user actions across all sections.",
            cx,
        )
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .px_4()
                .py_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{} entries", log_count)),
                )
                .when(has_logs, |el| {
                    el.child(
                        Button::new("clear-logs")
                            .label("Clear Logs")
                            .compact()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.activity_log.clear();
                                cx.notify();
                            })),
                    )
                }),
        );

        if self.activity_log.is_empty() {
            card.child(
                div().px_4().pb_3().child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("No activity yet. Click a button above."),
                ),
            )
        } else {
            // Virtualized list: only renders visible entries (20px each).
            let item_count = self.activity_log.len();
            let item_height = px(20.);
            let item_sizes = uniform_item_sizes(item_count, item_height);

            card.child(
                div()
                    .px_4()
                    .max_h(px(200.))
                    // Block scroll events from bubbling to the outer page scroll.
                    .on_scroll_wheel(|_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(render_virtual_list(
                        cx,
                        "activity-log-vlist",
                        item_sizes,
                        px(200.),
                        px(0.),
                        &scroll_handle,
                        false,
                        move |this: &mut Self, visible_range, _window, cx| {
                            // Render entries in reverse (newest first).
                            let total = this.activity_log.len();
                            visible_range
                                .map(|ix| {
                                    let entry_ix = total - 1 - ix;
                                    let entry = this
                                        .activity_log
                                        .get(entry_ix)
                                        .cloned()
                                        .unwrap_or_default();
                                    div()
                                        .h(px(20.))
                                        .text_xs()
                                        .font_family("monospace")
                                        .text_color(cx.theme().muted_foreground)
                                        .child(entry)
                                        .into_any_element()
                                })
                                .collect()
                        },
                    )),
            )
        }
    }
}
