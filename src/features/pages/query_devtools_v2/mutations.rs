use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};

use gpui_query_v2::client::ClientDiagnostic;
use gpui_query_v2::core::MutationStatus;

use super::dashboard::QueryDevToolsV2Page;

// ---------------------------------------------------------------------------
// Mutations Table
// ---------------------------------------------------------------------------

pub(super) fn render_mutations_table(
    diagnostic: &Option<ClientDiagnostic>,
    cx: &mut Context<QueryDevToolsV2Page>,
) -> Div {
    let theme = cx.theme();
    let radius_lg = theme.radius_lg;
    let border = theme.border;
    let muted = theme.muted;
    let muted_foreground = theme.muted_foreground;
    let primary = theme.primary;
    let danger = theme.danger;
    let radius = theme.radius;
    let _ = theme;

    let mutations: Vec<_> = diagnostic
        .as_ref()
        .map(|d| d.mutations.clone())
        .unwrap_or_default();

    // Header
    let header = h_flex().gap_3().px_3().py_2().children(vec![
        div()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .flex_1()
            .child("Key"),
        div()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .flex_1()
            .child("Status"),
        div()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .flex_1()
            .child("Retry Count"),
    ]);

    if mutations.is_empty() {
        return div()
            .rounded(radius_lg)
            .border_1()
            .border_color(border)
            .bg(muted)
            .p_4()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .mb_2()
                    .child("Mutations"),
            )
            .child(
                div().py_4().flex().justify_center().child(
                    div()
                        .text_sm()
                        .text_color(muted_foreground)
                        .child("No mutations registered."),
                ),
            );
    }

    let rows: Vec<_> = mutations
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let key_display = m.key.as_deref().unwrap_or("anonymous");

            let status_color = match m.status {
                MutationStatus::Idle => muted_foreground,
                MutationStatus::Loading => primary,
                MutationStatus::Success => primary,
                MutationStatus::Failure => danger,
            };

            let status_label = m.status.label();

            // Audit Finding 3: use a stable identifier combining key and index
            // instead of just the enumeration index to avoid ID shifts on removal.
            let row_id = format!(
                "v2-mutation-row-{}-{}",
                m.key.as_deref().unwrap_or("anon"),
                i
            );

            div()
                .id(ElementId::Name(SharedString::from(row_id)))
                .rounded(radius)
                .px_3()
                .py_2()
                .child(h_flex().gap_3().items_center().children(vec![
                        div()
                            .text_sm()
                            .font_family("monospace")
                            .flex_1()
                            .child(key_display.to_string()),
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .px_1()
                            .text_color(status_color)
                            .child(status_label.to_string()),
                        div()
                            .text_xs()
                            .text_color(muted_foreground)
                            .child(format!("{}", m.retry_count)),
                    ]))
                .into_any_element()
        })
        .collect();

    let table = v_flex().gap_0p5().children(rows);

    div()
        .rounded(radius_lg)
        .border_1()
        .border_color(border)
        .bg(muted)
        .p_4()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .mb_2()
                .child("Mutations"),
        )
        .child(header)
        .child(table)
}
