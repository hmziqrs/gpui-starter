use std::time::Instant;

use gpui::{prelude::*, *};
use gpui_component::v_flex;

use crate::http_lab::{self, HttpLabAction, HttpLabState};

mod panels;
mod ui_primitives;

const RENDER_LOG: &str = "gpui_starter::http_lab::render";

pub struct HttpLabPage {
    _subscriptions: Vec<Subscription>,
}

impl HttpLabPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe_global_in::<HttpLabState>(window, |_, _, cx| {
            cx.notify();
        })];

        Self {
            _subscriptions: subscriptions,
        }
    }

    fn run_action(&mut self, action: HttpLabAction, cx: &mut Context<Self>) {
        tracing::info!(
            target: "gpui_starter::http_lab::ui",
            action = action.id(),
            "HTTP Lab action clicked"
        );
        tracing::info!(
            target: "gpui_starter::http_lab::ui",
            action = action.id(),
            "HTTP Lab scheduling GPUI entity task"
        );
        cx.spawn(async move |_this, cx| {
            tracing::info!(
                target: "gpui_starter::http_lab::ui",
                action = action.id(),
                "HTTP Lab GPUI entity task started"
            );

            let handle = match cx.update(|cx| http_lab::prepare_action(action, cx)) {
                Some(handle) => handle,
                None => {
                    tracing::info!(
                        target: "gpui_starter::http_lab::ui",
                        action = action.id(),
                        "HTTP Lab action deduplicated inside entity task"
                    );
                    return;
                }
            };

            tracing::info!(
                target: "gpui_starter::http_lab::ui",
                action = action.id(),
                "HTTP Lab action prepared inside entity task"
            );
            http_lab::execute_action(handle, cx).await;
            tracing::info!(
                target: "gpui_starter::http_lab::ui",
                action = action.id(),
                "HTTP Lab GPUI entity task finished"
            );
        })
        .detach();
        tracing::info!(
            target: "gpui_starter::http_lab::ui",
            action = action.id(),
            "HTTP Lab GPUI entity task scheduled"
        );
    }
}

impl Render for HttpLabPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = Instant::now();

        // Extract action bar data first — action_bar needs &mut Context for
        // cx.listener(), so it must be built outside any state borrow.
        let (active_count, action_meta) = http_lab::read_state(cx, |state| {
            let meta = HttpLabAction::all()
                .iter()
                .copied()
                .map(|action| {
                    let resource = state.resource(action);
                    (action, resource.is_loading(), resource.request_policy())
                })
                .collect::<Vec<_>>();
            (state.active_count(), meta)
        });
        let bar = panels::action_bar_from_meta(active_count, action_meta, cx);

        // Now borrow state immutably via read_state for all read-only panels.
        // These helpers only need &App (not &mut Context), so the borrow is safe.
        let (history_len, transition_len, selected_action, selected_status, view) =
            http_lab::read_state(cx, |state| {
                let selected_resource = state.selected_resource();
                let selected_action = state.selected_action;
                let selected_status = selected_resource.status();
                let history_len = state.history.len();
                let transition_len = state.transition_log.len();

                let view = v_flex()
                    .min_h_full()
                    .p_6()
                    .gap_5()
                    .child(panels::hero(&state, cx))
                    .child(bar)
                    .child(panels::tab_bar(&state))
                    .child(panels::resource_panel(
                        &state,
                        state.selected_action,
                        selected_resource,
                        cx,
                    ))
                    .child(panels::activity_panel(&state, cx));

                (history_len, transition_len, selected_action, selected_status, view)
            });

        tracing::info!(
            target: RENDER_LOG,
            elapsed_us = render_started.elapsed().as_micros() as u64,
            selected_action = selected_action.id(),
            selected_status = selected_status.label(),
            active_count,
            history_len,
            transition_len,
            "HTTP Lab render completed"
        );

        view
    }
}
