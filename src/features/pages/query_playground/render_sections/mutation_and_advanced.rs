use gpui::prelude::*;
use gpui::*;

use std::sync::Arc;

use gpui_component::{
    ActiveTheme as _, Disableable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    v_flex,
};

use gpui_query::core::{MutationStatus, QueryStatus};

use super::super::ui_helpers::{chip, mapped_preview, section_card, source_preview, status_badge};
use super::super::{PlaygroundPage, QueryPlaygroundPage};

impl QueryPlaygroundPage {
    pub(in super::super) fn render_mutation(&mut self, cx: &mut Context<Self>) -> Div {
        let m_status = self
            .mutation_entity
            .as_ref()
            .map_or(MutationStatus::Idle, |(e, _)| {
                e.read_with(cx, |r, _| r.status())
            });
        let m_data = self
            .mutation_entity
            .as_ref()
            .and_then(|(e, _)| e.read_with(cx, |r, _| r.data().cloned()));
        let m_error = self
            .mutation_entity
            .as_ref()
            .and_then(|(e, _)| e.read_with(cx, |r, _| r.error().cloned()));
        let m_vars = self
            .mutation_entity
            .as_ref()
            .and_then(|(e, _)| e.read_with(cx, |r, _| r.variables().cloned()));
        let m_loading = m_status == MutationStatus::Loading;

        let status_color = match m_status {
            MutationStatus::Idle => cx.theme().muted_foreground,
            MutationStatus::Loading => cx.theme().info,
            MutationStatus::Success => cx.theme().success,
            MutationStatus::Failure => cx.theme().danger,
        };

        section_card(
            "Mutation",
            "Text input for mutation variables. Mutate() fires async; mutate_with_callbacks() also logs on_success/on_error.",
            cx,
        )
        .child(
            h_flex().gap_2().items_center().px_4().py_2()
                // Finding 1/8: Replace static div with a proper editable Input
                // component that binds to mutation_input_state.
                .child(
                    div().min_w(px(200.))
                        .child(Input::new(&self.mutation_input_state))
                )
                .child(
                    Button::new("pg-mutate")
                        .primary()
                        .label(if m_loading { "Mutating..." } else { "Mutate" })
                        .disabled(m_loading)
                        .on_click(cx.listener(|this, _, _, cx| this.do_mutate(cx))),
                )
                .child(
                    Button::new("pg-mutate-cb")
                        .outline()
                        .label("Mutate with Callbacks")
                        .disabled(m_loading)
                        .on_click(cx.listener(|this, _, _, cx| this.do_mutate_with_callbacks(cx))),
                )
                .child(
                    Button::new("pg-mutate-reset")
                        .outline()
                        .label("Reset")
                        .on_click(cx.listener(|this, _, _, cx| this.reset_mutation(cx))),
                ),
        )
        .child(
            v_flex().gap_1().px_4().pb_3()
                .child(
                    h_flex().gap_3().items_center()
                        .child(
                            div()
                                .px_3()
                                .py_1()
                                .rounded(cx.theme().radius_lg)
                                .border_1()
                                .border_color(status_color)
                                .text_sm()
                                .text_color(status_color)
                                .child(m_status.label().to_string()),
                        )
                        .when_some(m_data, |el, d| {
                            el.child(chip(&format!("data: {}", d), cx.theme().background, cx))
                        })
                        .when_some(m_vars, |el, v| {
                            el.child(chip(&format!("vars: {}", v), cx.theme().background, cx))
                        })
                        .when_some(m_error, |el, e| {
                            el.child(
                                div()
                                    .px_3()
                                    .py_1()
                                    .rounded(cx.theme().radius_lg)
                                    .border_1()
                                    .border_color(cx.theme().danger)
                                    .text_sm()
                                    .text_color(cx.theme().danger)
                                    .child(format!("error: {}", e)),
                            )
                        }),
                ),
        )
    }

    pub(in super::super) fn render_infinite_query(&mut self, cx: &mut Context<Self>) -> Div {
        let page_count = self
            .infinite_entity
            .as_ref()
            .map_or(0, |(e, _)| e.read_with(cx, |r, _| r.page_count()));
        let inf_status = self
            .infinite_entity
            .as_ref()
            .map_or(QueryStatus::Idle, |(e, _)| {
                e.read_with(cx, |r, _| r.status())
            });
        let has_next = self
            .infinite_entity
            .as_ref()
            .map_or(false, |(e, _)| e.read_with(cx, |r, _| r.has_next_page()));
        let has_prev = self.infinite_entity.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| r.has_previous_page())
        });
        let loading = inf_status.is_loading();
        let pages: Vec<(usize, Arc<PlaygroundPage>)> = self
            .infinite_entity
            .as_ref()
            .map(|(e, _)| {
                let mut result = Vec::new();
                e.read_with(cx, |r, _| {
                    for (i, page) in r.pages().iter().enumerate() {
                        result.push((i, Arc::new(page.clone())));
                    }
                });
                result
            })
            .unwrap_or_default();

        section_card(
            "Infinite Query",
            "Paginated data with max_pages=3 so eviction is visible. Fetcher generates pages of 5 items each.",
            cx,
        )
        .child(
            h_flex().gap_2().flex_wrap().px_4().py_3()
                .child(
                    Button::new("pg-inf-next")
                        .primary()
                        .label("Load Next Page")
                        .disabled(loading || !has_next)
                        .on_click(cx.listener(|this, _, _, cx| this.load_next_page(cx))),
                )
                .child(
                    Button::new("pg-inf-prev")
                        .outline()
                        .label("Load Previous Page")
                        // Finding 3: Disable when has_prev is false, mirroring
                        // the 'Load Next Page' button's !has_next check.
                        .disabled(loading || !has_prev)
                        .on_click(cx.listener(|this, _, _, cx| this.load_prev_page(cx))),
                )
                .child(
                    Button::new("pg-inf-reset")
                        .outline()
                        .label("Reset")
                        .on_click(cx.listener(|this, _, _, cx| this.reset_infinite(cx))),
                )
                .child(status_badge(inf_status, cx))
                .child(chip(&format!("pages: {}/3 (max)", page_count), cx.theme().background, cx))
                .child(chip(&format!("has_next: {}", has_next), cx.theme().background, cx))
                .child(chip(&format!("has_prev: {}", has_prev), cx.theme().background, cx)),
        )
        .when(!pages.is_empty(), |el| {
            el.child(
                v_flex().gap_2().px_4().pb_3()
                    .children(pages.into_iter().map(|(idx, page)| {
                        div()
                            .px_3()
                            .py_2()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().muted)
                            .child(
                                v_flex().gap_1()
                                    .child(
                                        div().text_xs().font_weight(FontWeight::SEMIBOLD)
                                            .child(format!("Page {} (index {})", page.page_number, idx)),
                                    )
                                    .child(
                                        div().text_xs().text_color(cx.theme().muted_foreground)
                                            .child(page.items.join(", ")),
                                    ),
                            )
                    })),
            )
        })
    }

    pub(in super::super) fn render_select_transform(&mut self, cx: &mut Context<Self>) -> Div {
        let source_data = self
            .select_source
            .as_ref()
            .and_then(|e| e.read_with(cx, |r, _| r.data().cloned()));
        let mapped_data = self
            .select_mapped
            .as_ref()
            .and_then(|e| e.read_with(cx, |r, _| r.data()));
        let source_status = self
            .select_source
            .as_ref()
            .map_or(QueryStatus::Idle, |e| e.read_with(cx, |r, _| r.status()));
        let loading = source_status.is_loading();

        section_card(
            "Select Transform",
            "Source query returns Vec<PlaygroundUser>. Transform projects to Vec<String> (names only).",
            cx,
        )
        .child(
            h_flex().gap_2().px_4().py_3()
                .child(
                    Button::new("pg-select-fetch")
                        .primary()
                        .label(if loading { "Fetching..." } else { "Fetch Source" })
                        .disabled(loading)
                        .on_click(cx.listener(|this, _, _, cx| this.fetch_select(cx))),
                )
                .child(status_badge(source_status, cx)),
        )
        .child(
            h_flex().gap_4().px_4().pb_3()
                .child(
                    v_flex().gap_1()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Source (Vec<User>)"))
                        .child(
                            div()
                                .p_2()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().muted)
                                .text_sm()
                                .child(source_preview(&source_data)),
                        ),
                )
                .child(
                    v_flex().gap_1()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Mapped (Vec<String>)"))
                        .child(
                            div()
                                .p_2()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().muted)
                                .text_sm()
                                .child(mapped_preview(&mapped_data)),
                        ),
                ),
        )
    }

    pub(in super::super) fn render_imperative_fetch(&mut self, cx: &mut Context<Self>) -> Div {
        let status = self
            .imperative_query
            .as_ref()
            .map_or(QueryStatus::Idle, |(e, _)| {
                e.read_with(cx, |r, _| r.status())
            });
        let data = self
            .imperative_query
            .as_ref()
            .and_then(|(e, _)| e.read_with(cx, |r, _| r.data().cloned()));
        let signal_cancelled = self.imperative_query.as_ref().map_or(false, |(e, _)| {
            e.read_with(cx, |r, _| {
                r.signal().map(|s| s.is_cancelled()).unwrap_or(false)
            })
        });
        let loading = status.is_loading();

        section_card(
            "Imperative Fetch",
            "Manual refetch with signal. Cancel mid-flight to observe cooperative cancellation.",
            cx,
        )
        .child(
            h_flex()
                .gap_2()
                .flex_wrap()
                .px_4()
                .py_3()
                .child(
                    Button::new("pg-imp-fetch")
                        .primary()
                        .label(if loading { "Fetching..." } else { "Fetch" })
                        .disabled(loading)
                        .on_click(cx.listener(|this, _, _, cx| this.fetch_imperative(cx))),
                )
                .child(
                    Button::new("pg-imp-cancel")
                        .outline()
                        .label("Cancel mid-flight")
                        .disabled(!loading)
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_imperative(cx))),
                )
                .child(
                    Button::new("pg-imp-reset")
                        .outline()
                        .label("Reset")
                        .on_click(cx.listener(|this, _, _, cx| this.reset_imperative(cx))),
                ),
        )
        .child(
            h_flex()
                .gap_3()
                .items_center()
                .px_4()
                .pb_3()
                .child(status_badge(status, cx))
                .when_some(data, |el, d| el.child(chip(&d, cx.theme().background, cx)))
                .child(chip(
                    &format!("signal cancelled: {}", signal_cancelled),
                    cx.theme().background,
                    cx,
                )),
        )
    }
}
