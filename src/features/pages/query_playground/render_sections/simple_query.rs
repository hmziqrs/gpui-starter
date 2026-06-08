use gpui::prelude::*;
use gpui::*;

use gpui_component::{
    ActiveTheme as _,
    Disableable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
};

use gpui_query_v2::core::QueryStatus;

use super::super::QueryPlaygroundPage;
use super::super::ui_helpers::{section_card, status_badge, chip};

impl QueryPlaygroundPage {
    pub(in super::super) fn render_simple_query(&mut self, cx: &mut Context<Self>) -> Div {
        let loading = self.simple_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.status().is_loading())
        });
        let status = self.simple_query.as_ref().map_or(QueryStatus::Idle, |(e, _)| {
            e.read_with(cx, |r, _| r.status())
        });
        let data_preview = self.simple_query.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| r.data().cloned())
        });
        let cache_age = self.simple_query.as_ref().and_then(|(e, _)| {
            e.read_with(cx, |r, _| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                r.last_updated_at_ms().map(|t| now.saturating_sub(t))
            })
        });
        let bg = cx.theme().background;

        section_card("Simple Query", "Basic fetch with NoCache + LatestWins. Simulates 1s async work.", cx)
            .child(
                h_flex().gap_2().flex_wrap().px_4().py_3()
                    .child(
                        Button::new("pg-simple-fetch")
                            // Finding 11: Use .primary() for the main action button.
                            .primary()
                            .label(if loading { "Fetching..." } else { "Fetch" })
                            .disabled(loading)
                            .on_click(cx.listener(|this, _, _, cx| this.fetch_simple(cx))),
                    )
                    .child(
                        Button::new("pg-simple-cancel")
                            .outline()
                            .label("Cancel")
                            .disabled(!loading)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_simple(cx))),
                    )
                    .child(
                        Button::new("pg-simple-reset")
                            .outline()
                            .label("Reset")
                            .on_click(cx.listener(|this, _, _, cx| this.reset_simple(cx))),
                    ),
            )
            .child(
                h_flex().gap_3().items_center().px_4().pb_3()
                    .child(status_badge(status, cx))
                    .when_some(data_preview, |el, user| {
                        el.child(chip(&format!("{} <{}>", user.name, user.email), bg, cx))
                    })
                    .when_some(cache_age, |el, age| {
                        el.child(chip(&format!("age: {}ms", age), bg, cx))
                    }),
            )
    }
}
