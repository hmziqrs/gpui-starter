use std::rc::Rc;

use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme as _, VirtualListScrollHandle, h_flex, v_flex};

use crate::ui::widgets::{bounded_list_height, render_virtual_list, variable_item_sizes};
use gpui_query::client::ClientDiagnostic;
use gpui_query::core::QueryStatus;

use super::dashboard::QueryDevToolsV2Page;
use super::helpers::{QuerySort, filter_button, format_cache_age, rems_from_px, sort_button};

// ---------------------------------------------------------------------------
// Virtual-list geometry constants
// ---------------------------------------------------------------------------

// `v_virtual_list` positions items purely from the `item_sizes` we hand it, so
// the rendered row/detail elements are pinned to these exact heights — otherwise
// rows would overlap or leave gaps. Keep these in sync with `query_row` /
// `query_expanded_detail` if their styling changes.
const REGISTRY_ROW_H: f32 = 38.0; // px_3/py_2 single-line row
const REGISTRY_DETAIL_H: f32 = 156.0; // 6-field expanded detail (p_3 + gap_1)
const REGISTRY_ITEM_GAP: f32 = 4.0; // gap_1 between row and detail (expanded)
const REGISTRY_LIST_GAP: f32 = 2.0; // gap_0p5 between registry items
const REGISTRY_MAX_LIST_H: f32 = 480.0; // cap before the list itself scrolls

// ---------------------------------------------------------------------------
// Query Registry (sort/filter controls + table)
// ---------------------------------------------------------------------------

pub(super) fn render_query_registry(
    diagnostic: &Option<ClientDiagnostic>,
    expanded_key: &Option<String>,
    sort_by: QuerySort,
    status_filter: &Option<String>,
    scroll_handle: &VirtualListScrollHandle,
    cx: &mut Context<QueryDevToolsV2Page>,
) -> Div {
    let theme = cx.theme();
    let radius_lg = theme.radius_lg;
    let border = theme.border;
    let muted = theme.muted;
    let muted_foreground = theme.muted_foreground;
    let _ = theme;

    // Sort controls
    let sort_controls = h_flex().gap_2().children(vec![
        sort_button("By Key", QuerySort::Key, sort_by, cx),
        sort_button("By Status", QuerySort::Status, sort_by, cx),
        sort_button("By Cache Age", QuerySort::CacheAge, sort_by, cx),
        sort_button("By Cache Hits", QuerySort::CacheHits, sort_by, cx),
    ]);

    // Status filter options matching v2 QueryStatus variants
    let status_options: Vec<Option<&str>> = vec![
        None,
        Some("Idle"),
        Some("LoadingEmpty"),
        Some("LoadingWithData"),
        Some("Success"),
        Some("Failure"),
        Some("Cancelled"),
    ];

    let filter_controls = h_flex().gap_2().children(
        status_options
            .into_iter()
            .map(|opt| filter_button(opt, status_filter, cx)),
    );

    // Build sorted/filtered query rows
    let mut queries: Vec<_> = diagnostic
        .as_ref()
        .map(|d| d.queries.clone())
        .unwrap_or_default();

    // Apply status filter (Audit Finding 1: log warning on unknown status strings).
    if let Some(filter) = status_filter {
        let filter_status = match filter.as_str() {
            "Idle" => Some(QueryStatus::Idle),
            "LoadingEmpty" => Some(QueryStatus::LoadingEmpty),
            "LoadingWithData" => Some(QueryStatus::LoadingWithData),
            "Success" => Some(QueryStatus::Success),
            "Failure" => Some(QueryStatus::Failure),
            "Cancelled" => Some(QueryStatus::Cancelled),
            _ => {
                tracing::warn!(
                    "QueryDevToolsV2: unknown status_filter value {:?}; clearing filter",
                    filter
                );
                None
            }
        };
        if let Some(fs) = filter_status {
            queries.retain(|q| q.status == fs);
        }
    }

    // Apply sort
    match sort_by {
        QuerySort::Key => queries.sort_by(|a, b| a.key.cmp(&b.key)),
        // Audit Finding 2: use semantic Ord ordering (declaration order) instead
        // of fragile Debug string comparison.
        QuerySort::Status => queries.sort_by(|a, b| a.status.cmp(&b.status)),
        QuerySort::CacheAge => queries.sort_by(|a, b| {
            b.cache_age_ms
                .unwrap_or(0)
                .cmp(&a.cache_age_ms.unwrap_or(0))
        }),
        QuerySort::CacheHits => queries.sort_by(|a, b| b.cache_hits.cmp(&a.cache_hits)),
    }

    // Header row
    let header = h_flex().gap_3().px_3().py_2().children(vec![
        div().w(rems_from_px(16.0)).child(div()),
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
            .child("Cache Policy"),
        div()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .flex_1()
            .child("Cache Age"),
        div()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .flex_1()
            .child("Cache Hits"),
        div()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .flex_1()
            .child("Retry Count"),
    ]);

    // Virtualize the registry: only the rows in the visible range are laid out
    // and painted, so adding queries no longer forces a full-tree relayout on
    // every scroll frame.
    let queries = Rc::new(queries);

    let item_heights: Vec<Pixels> = queries
        .iter()
        .map(|q| {
            let is_expanded = expanded_key.as_deref() == Some(q.key.as_str());
            if is_expanded {
                px(REGISTRY_ROW_H + REGISTRY_ITEM_GAP + REGISTRY_DETAIL_H)
            } else {
                px(REGISTRY_ROW_H)
            }
        })
        .collect();

    let item_sizes = variable_item_sizes(&item_heights);
    let list_h = bounded_list_height(&item_sizes, px(REGISTRY_LIST_GAP), px(REGISTRY_MAX_LIST_H));

    // Empty state within registry
    let registry_content = if queries.is_empty() {
        div().py_6().flex().justify_center().child(
            div()
                .text_sm()
                .text_color(muted_foreground)
                .child("No queries match the current filter."),
        )
    } else {
        let scroll_handle = scroll_handle.clone();
        let rows = queries.clone();
        let expanded = expanded_key.clone();

        render_virtual_list(
            cx,
            "v2-registry-rows",
            item_sizes,
            list_h,
            px(REGISTRY_LIST_GAP),
            &scroll_handle,
            true,
            move |_this, range, _window, cx| {
                range
                    .map(|ix| {
                        let q = &rows[ix];
                        let is_expanded = expanded.as_deref() == Some(q.key.as_str());
                        // Drop the theme borrow before query_row takes `&mut cx`.
                        let (radius, secondary) = {
                            let theme = cx.theme();
                            (theme.radius, theme.secondary)
                        };
                        let mut item = v_flex()
                            .gap_1()
                            .child(query_row(q, is_expanded, cx).h(px(REGISTRY_ROW_H)));
                        if is_expanded {
                            item = item.child(
                                query_expanded_detail(q, radius, secondary)
                                    .h(px(REGISTRY_DETAIL_H)),
                            );
                        }
                        item
                    })
                    .collect::<Vec<_>>()
            },
        )
    };

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
                .child("Query Registry"),
        )
        .child(
            v_flex()
                .gap_2()
                .mb_3()
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_xs().text_color(muted_foreground).child("Sort:"))
                        .child(sort_controls),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted_foreground)
                                .child("Status:"),
                        )
                        .child(filter_controls),
                ),
        )
        .child(header)
        .child(registry_content)
}

// ---------------------------------------------------------------------------
// Query Row
// ---------------------------------------------------------------------------

fn query_row(
    q: &gpui_query::client::QueryDiagnostic,
    is_expanded: bool,
    cx: &mut Context<QueryDevToolsV2Page>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let muted_foreground = theme.muted_foreground;
    let primary = theme.primary;
    let danger = theme.danger;
    let secondary = theme.secondary;
    let radius = theme.radius;
    let _ = theme;

    let key = q.key.clone();

    // Audit Finding 17: use plain colored text for status (no filled badge
    // background), matching the existing query_devtools.rs rendering style.
    let status_color = match q.status {
        QueryStatus::Idle | QueryStatus::Cancelled => muted_foreground,
        QueryStatus::LoadingEmpty | QueryStatus::LoadingWithData => primary,
        QueryStatus::Success => primary,
        QueryStatus::Failure => danger,
    };

    let status_label = match q.status {
        QueryStatus::Idle => "Idle",
        QueryStatus::LoadingEmpty => "Loading",
        QueryStatus::LoadingWithData => "Loading",
        QueryStatus::Success => "Success",
        QueryStatus::Failure => "Failure",
        QueryStatus::Cancelled => "Cancelled",
    };

    // Audit Finding 14: use proper Unicode arrows matching query_devtools.rs.
    let chevron = if is_expanded { "\u{25BE}" } else { "\u{25B8}" };

    let cache_age_str = format_cache_age(q.cache_age_ms);

    div()
        .id(SharedString::from(format!("v2-query-row-{}", q.key)))
        .cursor_pointer()
        .rounded(radius)
        .px_3()
        .py_2()
        .hover(move |s| s.bg(secondary))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.expanded_key = if this.expanded_key.as_deref() == Some(key.as_str()) {
                None
            } else {
                Some(key.clone())
            };
            cx.notify();
        }))
        .child(
            // Audit Finding 16: chevron column uses explicit width matching the
            // header placeholder so the 6 flex_1 data columns align properly.
            h_flex().gap_3().items_center().children(vec![
                div()
                    .w(rems_from_px(16.0))
                    .text_xs()
                    .text_color(muted_foreground)
                    .child(chevron.to_string()),
                div()
                    .text_sm()
                    .font_family("monospace")
                    .flex_1()
                    .child(q.key.clone()),
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .px_1()
                    .text_color(status_color)
                    .child(status_label.to_string()),
                div()
                    .text_xs()
                    .px_1()
                    .rounded(radius)
                    .bg(secondary)
                    .child(q.cache_policy.clone()),
                div()
                    .text_xs()
                    .text_color(muted_foreground)
                    .child(cache_age_str),
                div()
                    .text_xs()
                    .text_color(muted_foreground)
                    .child(format!("{}", q.cache_hits)),
                div()
                    .text_xs()
                    .text_color(muted_foreground)
                    .child(format!("{}", q.retry_count)),
            ]),
        )
}

// ---------------------------------------------------------------------------
// Query Expanded Detail
// ---------------------------------------------------------------------------

fn query_expanded_detail(
    q: &gpui_query::client::QueryDiagnostic,
    radius: Pixels,
    secondary: Hsla,
) -> Div {
    let fields: Vec<(&str, String)> = vec![
        ("Key", q.key.clone()),
        ("Status", format!("{:?}", q.status)),
        ("Cache Policy", q.cache_policy.clone()),
        ("Cache Age", format_cache_age(q.cache_age_ms)),
        ("Cache Hits", format!("{}", q.cache_hits)),
        ("Retry Count", format!("{}", q.retry_count)),
    ];

    // Audit Finding 15: border_1() requires an explicit border_color for
    // proper theme-aware visual delineation.
    div()
        .ml_4()
        .border_1()
        .border_color(secondary)
        .rounded(radius)
        .p_3()
        .gap_1()
        .children(fields.into_iter().map(|(label, value)| {
            h_flex().gap_2().child(
                div().text_xs().child(
                    h_flex()
                        .gap_1()
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .child(format!("{}:", label)),
                        )
                        .child(div().child(value)),
                ),
            )
        }))
}
