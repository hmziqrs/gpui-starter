mod rows;

use gpui::{prelude::*, *};
use gpui_component::{button::Button, v_flex};

use crate::{
    app_state, capabilities, crash_report, desktop_actions, error_surface, notifications, shortcuts, storage, telemetry, undo_stack, accessibility,
    lifecycle::LifecycleState,
};

pub struct DiagnosticsPage {
    _subscriptions: Vec<Subscription>,
}

impl DiagnosticsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = Vec::new();
        subscriptions.push(
            cx.observe_global_in::<app_state::AppState>(window, |_, _, cx| {
                cx.notify();
            }),
        );
        subscriptions.push(cx.observe_global_in::<LifecycleState>(window, |_, _, cx| {
            cx.notify();
        }));
        subscriptions.push(
            cx.observe_global_in::<notifications::NativeNotificationState>(window, |_, _, cx| {
                cx.notify();
            }),
        );
        subscriptions.push(cx.observe_global_in::<capabilities::CapabilityRegistry>(
            window,
            |_, _, cx| {
                cx.notify();
            },
        ));
        subscriptions.push(
            cx.observe_global_in::<storage::StorageSnapshot>(window, |_, _, cx| {
                cx.notify();
            }),
        );
        subscriptions.push(cx.observe_global_in::<telemetry::TelemetrySnapshot>(
            window,
            |_, _, cx| {
                cx.notify();
            },
        ));
        subscriptions.push(
            cx.observe_global_in::<desktop_actions::DesktopActionsState>(window, |_, _, cx| {
                cx.notify();
            }),
        );
        subscriptions.push(
            cx.observe_global_in::<shortcuts::ShortcutState>(window, |_, _, cx| {
                cx.notify();
            }),
        );
        subscriptions.push(
            cx.observe_global_in::<undo_stack::UndoState>(window, |_, _, cx| {
                cx.notify();
            }),
        );
        subscriptions.push(
            cx.observe_global_in::<accessibility::AccessibilitySnapshot>(window, |_, _, cx| {
                cx.notify();
            }),
        );
        subscriptions.push(cx.observe_global_in::<error_surface::ErrorSurfaceState>(
            window,
            |_, _, cx| {
                cx.notify();
            },
        ));
        subscriptions.push(cx.observe_global_in::<crash_report::CrashReportSnapshot>(
            window,
            |_, _, cx| {
                cx.notify();
            },
        ));
        Self {
            _subscriptions: subscriptions,
        }
    }
}

impl Render for DiagnosticsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = rows::build_diagnostic_rows(cx);

        v_flex()
            .min_h_full()
            .p_6()
            .gap_3()
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .child("Diagnostics"),
            )
            .child(
                Button::new("diagnostics-refresh")
                    .outline()
                    .label("Refresh")
                    .on_click(|_, _, cx| {
                        crate::events::emit(crate::events::AppEventKind::DiagnosticsChanged, cx);
                    }),
            )
            .child(
                Button::new("diagnostics-reset-first-run")
                    .outline()
                    .label("Reset First-Run")
                    .on_click(|_, _, cx| {
                        crate::first_run::reset(cx);
                    }),
            )
            .child(
                Button::new("diagnostics-copy")
                    .outline()
                    .label("Copy Diagnostics")
                    .on_click(|_, _, cx| {
                        let _ = crate::desktop_actions::copy_diagnostics(cx);
                    }),
            )
            .child(
                Button::new("diagnostics-open-logs")
                    .outline()
                    .label("Open Logs Folder")
                    .on_click(|_, _, cx| {
                        let _ = crate::desktop_actions::open_logs_folder(cx);
                    }),
            )
            .child(
                Button::new("diagnostics-dismiss-latest-error")
                    .outline()
                    .label("Dismiss Latest Error")
                    .on_click(|_, _, cx| {
                        if let Some(error) = crate::error_surface::latest(cx) {
                            crate::error_surface::dismiss(error.id, cx);
                        }
                    }),
            )
            .child(
                Button::new("diagnostics-retry-crash-upload")
                    .outline()
                    .label("Retry Crash Upload")
                    .on_click(|_, _, cx| {
                        crate::crash_report::upload_pending_reports(cx);
                    }),
            )
            .when(cfg!(debug_assertions), |this| {
                this.child(
                    Button::new("diagnostics-trigger-test-panic")
                        .outline()
                        .label("Trigger Test Panic")
                        .on_click(|_, _, cx| {
                            cx.dispatch_action(&crate::app::TriggerTestPanic);
                        }),
                )
            })
            .children(rows)
    }
}

fn row(label: &str, value: &str) -> Div {
    div().child(
        div()
            .flex()
            .gap_2()
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child(format!("{label}:")),
            )
            .child(div().child(value.to_string())),
    )
}
