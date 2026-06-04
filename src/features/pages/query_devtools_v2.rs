use std::rc::Rc;

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Disableable, Selectable,
    Icon, IconName,
    VirtualListScrollHandle,
    button::Button,
    h_flex,
    scroll::{ScrollableElement, ScrollbarAxis},
    v_flex, v_virtual_list,
};
use gpui_query_v2::client::{ClientDiagnostic, QueryClient};
use gpui_query_v2::core::{MutationStatus, QueryKeyFilter, QueryStatus};

// ---------------------------------------------------------------------------
// Sort mode
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuerySort {
    Key,
    Status,
    CacheAge,
    CacheHits,
}

// ---------------------------------------------------------------------------
// Query DevTools V2 Page
// ---------------------------------------------------------------------------

pub struct QueryDevToolsV2Page {
    _subscriptions: Vec<Subscription>,
    expanded_key: Option<String>,
    sort_by: QuerySort,
    /// Status filter: `None` means "show all", `Some(String)` must be a valid
    /// `QueryStatus` variant name (e.g. "Idle", "Success"). See Audit Finding 4.
    status_filter: Option<String>,
    /// Scroll handle for the virtualized query registry list.
    scroll_handle: VirtualListScrollHandle,
}

impl QueryDevToolsV2Page {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = Vec::new();
        subscriptions.push(
            cx.observe_global_in::<QueryClient>(window, |_, _, cx| {
                cx.notify();
            }),
        );
        Self {
            _subscriptions: subscriptions,
            expanded_key: None,
            sort_by: QuerySort::Key,
            status_filter: None,
            scroll_handle: VirtualListScrollHandle::new(),
        }
    }
}

impl Render for QueryDevToolsV2Page {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let diagnostic = cx.try_global::<QueryClient>().map(|c| c.diagnostics(cx));

        let scroll_handle = self.scroll_handle.clone();
        let content = if diagnostic.is_some() {
            render_dashboard(
                &diagnostic,
                &self.expanded_key,
                self.sort_by,
                &self.status_filter,
                &scroll_handle,
                cx,
            )
        } else {
            render_empty_state(cx)
        };

        v_flex().min_h_full().p_6().gap_5().child(content)
    }
}

// ---------------------------------------------------------------------------
// Empty State
// ---------------------------------------------------------------------------

fn render_empty_state(cx: &mut Context<QueryDevToolsV2Page>) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .py_12()
        .child(
            Icon::new(IconName::Inbox)
                .size_10()
                .text_color(theme.muted_foreground),
        )
        .child(
            div()
                .text_xl()
                .font_weight(FontWeight::BOLD)
                .child("No V2 Query Resources"),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("Navigate to the Query Playground page to create queries, then return here."),
        )
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

fn render_dashboard(
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

    // Use actual collected lengths (skips dead/GC'd entries) rather than
    // bucket.count() which includes stale weak references (Audit Finding 7).
    let query_count = diagnostic.as_ref().map(|d| d.queries.len()).unwrap_or(0);
    let mutation_count = diagnostic.as_ref().map(|d| d.mutations.len()).unwrap_or(0);
    let cache_entries = diagnostic
        .as_ref()
        .map(|d| {
            d.queries
                .iter()
                .filter(|q| q.status == QueryStatus::Success)
                .count()
        })
        .unwrap_or(0);
    let failed_queries = diagnostic
        .as_ref()
        .map(|d| {
            d.queries
                .iter()
                .filter(|q| q.status == QueryStatus::Failure)
                .count()
        })
        .unwrap_or(0);

    // Hero banner
    let hero = div()
        .rounded(radius_lg)
        .border_1()
        .border_color(border)
        .bg(muted)
        .p_4()
        .child(
            div()
                .text_xl()
                .font_weight(FontWeight::BOLD)
                .child("Query V2 DevTools"),
        )
        .child(
            div()
                .text_sm()
                .text_color(muted_foreground)
                .child("Live diagnostics dashboard for gpui-query-v2's QueryClient."),
        );

    // Overview cards (4 in a row)
    let overview = h_flex().gap_4().children(vec![
        stat_card(
            "Total Queries",
            query_count.to_string(),
            radius_lg,
            border,
            muted,
            muted_foreground,
        ),
        stat_card(
            "Total Mutations",
            mutation_count.to_string(),
            radius_lg,
            border,
            muted,
            muted_foreground,
        ),
        stat_card(
            "Cache Entries",
            cache_entries.to_string(),
            radius_lg,
            border,
            muted,
            muted_foreground,
        ),
        stat_card(
            "Failed Queries",
            failed_queries.to_string(),
            radius_lg,
            border,
            muted,
            muted_foreground,
        ),
    ]);

    // Action bar (5 buttons)
    let actions = render_action_bar(cx);

    // Query registry table
    let registry =
        render_query_registry(diagnostic, expanded_key, sort_by, status_filter, scroll_handle, cx);

    // Mutations table
    let mutations = render_mutations_table(diagnostic, cx);

    div()
        .gap_5()
        .flex()
        .flex_col()
        .child(hero)
        .child(overview)
        .child(actions)
        .child(registry)
        .child(mutations)
}

// ---------------------------------------------------------------------------
// Stat Card
// ---------------------------------------------------------------------------

fn stat_card(
    label: &str,
    value: String,
    radius_lg: Pixels,
    border: Hsla,
    muted: Hsla,
    muted_foreground: Hsla,
) -> Div {
    div()
        .flex_1()
        .rounded(radius_lg)
        .border_1()
        .border_color(border)
        .bg(muted)
        .p_4()
        .child(div().text_2xl().font_weight(FontWeight::BOLD).child(value))
        .child(
            div()
                .text_sm()
                .text_color(muted_foreground)
                .child(label.to_string()),
        )
}

// ---------------------------------------------------------------------------
// Action Bar
// ---------------------------------------------------------------------------

fn render_action_bar(cx: &mut Context<QueryDevToolsV2Page>) -> Div {
    let theme = cx.theme();
    let radius_lg = theme.radius_lg;
    let border = theme.border;
    let muted = theme.muted;
    let _ = theme;

    let has_client = cx.has_global::<QueryClient>();

    let invalidate = Button::new("v2-devtools-invalidate-all")
        .outline()
        .label("Invalidate All")
        .when(!has_client, |btn| btn.disabled(true))
        .on_click(cx.listener(|_, _, _, cx| {
            if cx.has_global::<QueryClient>() {
                cx.update_global::<QueryClient, _>(|client, cx| {
                    client.invalidate_queries(&QueryKeyFilter::All, cx);
                });
                cx.notify();
            }
        }));

    let reset = Button::new("v2-devtools-reset-all")
        .outline()
        .label("Reset All")
        .when(!has_client, |btn| btn.disabled(true))
        .on_click(cx.listener(|_, _, _, cx| {
            if cx.has_global::<QueryClient>() {
                cx.update_global::<QueryClient, _>(|client, cx| {
                    client.reset_queries(&QueryKeyFilter::All, cx);
                });
                cx.notify();
            }
        }));

    let gc = Button::new("v2-devtools-gc")
        .outline()
        .label("GC")
        .when(!has_client, |btn| btn.disabled(true))
        .on_click(cx.listener(|_, _, _, cx| {
            if cx.has_global::<QueryClient>() {
                cx.update_global::<QueryClient, _>(|client, cx| {
                    client.gc(cx);
                });
                cx.notify();
            }
        }));

    let cancel = Button::new("v2-devtools-cancel-all")
        .outline()
        .label("Cancel All")
        .when(!has_client, |btn| btn.disabled(true))
        .on_click(cx.listener(|_, _, _, cx| {
            if cx.has_global::<QueryClient>() {
                cx.update_global::<QueryClient, _>(|client, cx| {
                    client.cancel_queries(&QueryKeyFilter::All, cx);
                });
                cx.notify();
            }
        }));

    let remove = Button::new("v2-devtools-remove-all")
        .outline()
        .label("Remove All")
        .when(!has_client, |btn| btn.disabled(true))
        .on_click(cx.listener(|_, _, _, cx| {
            if cx.has_global::<QueryClient>() {
                cx.update_global::<QueryClient, _>(|client, _cx| {
                    client.remove_queries(&QueryKeyFilter::All);
                });
                cx.notify();
            }
        }));

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
                .child("Actions"),
        )
        .child(
            h_flex()
                .gap_2()
                .flex_wrap()
                .children(vec![invalidate, reset, gc, cancel, remove]),
        )
}

// ---------------------------------------------------------------------------
// Query Registry (sort/filter controls + table)
// ---------------------------------------------------------------------------

// Virtual-list geometry. `v_virtual_list` positions items purely from the
// `item_sizes` we hand it, so the rendered row/detail elements are pinned to
// these exact heights — otherwise rows would overlap or leave gaps. Keep these
// in sync with `query_row` / `query_expanded_detail` if their styling changes.
const REGISTRY_ROW_H: f32 = 38.0; // px_3/py_2 single-line row
const REGISTRY_DETAIL_H: f32 = 156.0; // 6-field expanded detail (p_3 + gap_1)
const REGISTRY_ITEM_GAP: f32 = 4.0; // gap_1 between row and detail (expanded)
const REGISTRY_LIST_GAP: f32 = 2.0; // gap_0p5 between registry items
const REGISTRY_MAX_LIST_H: f32 = 480.0; // cap before the list itself scrolls

fn render_query_registry(
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
        QuerySort::CacheAge => {
            queries.sort_by(|a, b| {
                b.cache_age_ms.unwrap_or(0).cmp(&a.cache_age_ms.unwrap_or(0))
            })
        }
        QuerySort::CacheHits => queries.sort_by(|a, b| b.cache_hits.cmp(&a.cache_hits)),
    }

    // Header row
    let header = h_flex()
        .gap_3()
        .px_3()
        .py_2()
        .children(vec![
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
    // every scroll frame. `v_virtual_list` positions items from `item_sizes`,
    // so each rendered row/detail is pinned to the height declared here.
    let queries = Rc::new(queries);

    let item_heights: Vec<f32> = queries
        .iter()
        .map(|q| {
            let is_expanded = expanded_key.as_deref() == Some(q.key.as_str());
            if is_expanded {
                REGISTRY_ROW_H + REGISTRY_ITEM_GAP + REGISTRY_DETAIL_H
            } else {
                REGISTRY_ROW_H
            }
        })
        .collect();

    let item_sizes: Rc<Vec<Size<Pixels>>> =
        Rc::new(item_heights.iter().map(|&h| size(px(0.), px(h))).collect());

    // Snug height: the list only scrolls once content exceeds the cap.
    let content_h = item_heights.iter().sum::<f32>()
        + REGISTRY_LIST_GAP * item_heights.len().saturating_sub(1) as f32;
    let list_h = px(content_h.min(REGISTRY_MAX_LIST_H));

    // Empty state within registry
    let registry_content = if queries.is_empty() {
        div()
            .py_6()
            .flex()
            .justify_center()
            .child(
                div()
                    .text_sm()
                    .text_color(muted_foreground)
                    .child("No queries match the current filter."),
            )
    } else {
        let entity = cx.entity();
        let scroll_handle = scroll_handle.clone();
        let rows = queries.clone();
        let expanded = expanded_key.clone();

        let list = v_virtual_list(
            entity,
            "v2-registry-rows",
            item_sizes,
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
        .track_scroll(&scroll_handle)
        .gap_0p5();

        div().relative().w_full().h(list_h).child(
            v_flex()
                .id("v2-registry-list")
                .relative()
                .size_full()
                .child(list)
                .scrollbar(&scroll_handle, ScrollbarAxis::Vertical),
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
            v_flex().gap_2().mb_3().child(
                v_flex()
                    .gap_1()
                    .child(div().text_xs().text_color(muted_foreground).child("Sort:"))
                    .child(sort_controls),
            ).child(
                v_flex()
                    .gap_1()
                    .child(div().text_xs().text_color(muted_foreground).child("Status:"))
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
    q: &gpui_query_v2::client::QueryDiagnostic,
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
    q: &gpui_query_v2::client::QueryDiagnostic,
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

// ---------------------------------------------------------------------------
// Mutations Table
// ---------------------------------------------------------------------------

fn render_mutations_table(
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
    let header = h_flex()
        .gap_3()
        .px_3()
        .py_2()
        .children(vec![
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
                div()
                    .py_4()
                    .flex()
                    .justify_center()
                    .child(
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
                .child(
                    h_flex().gap_3().items_center().children(vec![
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
                    ]),
                )
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

// ---------------------------------------------------------------------------
// Sort Button
// ---------------------------------------------------------------------------

fn sort_button(
    label: &str,
    target: QuerySort,
    current: QuerySort,
    cx: &mut Context<QueryDevToolsV2Page>,
) -> Button {
    let active = current == target;
    Button::new(format!("v2-sort-{:?}", target))
        .outline()
        .label(label)
        .when(active, |btn| btn.selected(true))
        .on_click(cx.listener(move |this, _, _, _cx| {
            this.sort_by = target;
            this.expanded_key = None;
            _cx.notify();
        }))
}

// ---------------------------------------------------------------------------
// Filter Button
// ---------------------------------------------------------------------------

fn filter_button(
    target: Option<&str>,
    current: &Option<String>,
    cx: &mut Context<QueryDevToolsV2Page>,
) -> Button {
    let label = target.unwrap_or("All");
    let active = match (current, target) {
        (None, None) => true,
        (Some(cur), Some(tgt)) => cur == tgt,
        _ => false,
    };
    let id = format!("v2-filter-{}", target.unwrap_or("all"));
    let target_owned = target.map(|s| s.to_string());
    Button::new(id)
        .outline()
        .label(label)
        .when(active, |btn| btn.selected(true))
        .on_click(cx.listener(move |this, _, _, _cx| {
            this.status_filter = target_owned.clone();
            this.expanded_key = None;
            _cx.notify();
        }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_cache_age(age_ms: Option<u128>) -> String {
    match age_ms {
        None => "n/a".to_string(),
        Some(ms) => {
            if ms < 1000 {
                format!("{}ms", ms)
            } else if ms < 60_000 {
                format!("{:.1}s", ms as f64 / 1000.0)
            } else if ms < 3_600_000 {
                format!("{:.1}m", ms as f64 / 60_000.0)
            } else {
                format!("{:.1}h", ms as f64 / 3_600_000.0)
            }
        }
    }
}

/// Convert pixels to rems assuming a 16px base font size (Audit Finding 18:
/// this divisor matches GPUI's default but may differ with system config).
fn rems_from_px(px: f32) -> Rems {
    Rems(px / 16.0)
}
