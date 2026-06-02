use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Disableable, Selectable,
    Icon, IconName,
    button::Button,
    h_flex, v_flex,
};
use gpui_query::client::QueryClient;
use gpui_query::QueryKeyFilter;

use crate::services::http_lab::{HttpLabDiagnostic, HttpLabState};

// ---------------------------------------------------------------------------
// Sort mode
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuerySort {
    ByKey,
    ByStatus,
    ByCacheHits,
}

// ---------------------------------------------------------------------------
// Query DevTools Page
// ---------------------------------------------------------------------------

pub struct QueryDevToolsPage {
    _subscriptions: Vec<Subscription>,
    expanded_key: Option<String>,
    sort_by: QuerySort,
    status_filter: Option<String>,
}

impl QueryDevToolsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = Vec::new();
        subscriptions.push(
            cx.observe_global_in::<QueryClient>(window, |_, _, cx| {
                cx.notify();
            }),
        );
        subscriptions.push(
            cx.observe_global_in::<HttpLabState>(window, |_, _, cx| {
                cx.notify();
            }),
        );
        Self {
            _subscriptions: subscriptions,
            expanded_key: None,
            sort_by: QuerySort::ByKey,
            status_filter: None,
        }
    }
}

impl Render for QueryDevToolsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let client_diag = cx.try_global::<QueryClient>().map(|c| c.diagnostics(cx));
        let lab_diag = cx.try_global::<HttpLabState>()
            .map(|s| s.diagnostics());

        let content = if client_diag.is_some() || lab_diag.is_some() {
            render_dashboard(&client_diag, &lab_diag, &self.expanded_key, self.sort_by, &self.status_filter, cx)
        } else {
            render_empty_state(cx)
        };

        v_flex().min_h_full().p_6().gap_5().child(content)
    }
}

// ---------------------------------------------------------------------------
// Empty State
// ---------------------------------------------------------------------------

fn render_empty_state(cx: &mut Context<QueryDevToolsPage>) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .py_12()
        .child(
            Icon::new(IconName::Inbox).size_10().text_color(theme.muted_foreground),
        )
        .child(
            div()
                .text_xl()
                .font_weight(FontWeight::BOLD)
                .child("No Query Resources"),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("Navigate to HTTP Lab Testing to create queries, then return here."),
        )
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

fn render_dashboard(
    client_diag: &Option<gpui_query::client::devtools::ClientDiagnostic>,
    lab_diag: &Option<Vec<HttpLabDiagnostic>>,
    expanded_key: &Option<String>,
    sort_by: QuerySort,
    status_filter: &Option<String>,
    cx: &mut Context<QueryDevToolsPage>,
) -> Div {
    // Extract theme colors upfront to release the borrow on cx before
    // calling functions that need mutable access to cx.
    let theme = cx.theme();
    let radius_lg = theme.radius_lg;
    let border = theme.border;
    let muted = theme.muted;
    let muted_foreground = theme.muted_foreground;
    let _ = theme;

    // Compute totals across both sources
    let client_resources = client_diag.as_ref().map(|d| d.total_resources).unwrap_or(0);
    let lab_resources = lab_diag.as_ref().map(|d| d.len()).unwrap_or(0);
    let total_resources = client_resources + lab_resources;
    let bucket_count = client_diag.as_ref().map(|d| d.bucket_count).unwrap_or(0);
    let mutation_count = client_diag.as_ref().map(|d| d.mutation_count).unwrap_or(0);

    // Hero banner
    let hero = div()
        .rounded(radius_lg)
        .border_1()
        .border_color(border)
        .bg(muted)
        .p_4()
        .child(
            div().text_xl().font_weight(FontWeight::BOLD).child("Query DevTools"),
        )
        .child(
            div()
                .text_sm()
                .text_color(muted_foreground)
                .child("Live diagnostics dashboard for query resources."),
        );

    // Overview cards
    let overview = h_flex().gap_4().children(vec![
        stat_card("Total Resources", total_resources.to_string(), radius_lg, border, muted, muted_foreground),
        stat_card("Type Buckets", bucket_count.to_string(), radius_lg, border, muted, muted_foreground),
        stat_card("HTTP Lab Actions", lab_resources.to_string(), radius_lg, border, muted, muted_foreground),
        stat_card("Mutations", mutation_count.to_string(), radius_lg, border, muted, muted_foreground),
    ]);

    // Action bar
    let actions = render_action_bar(cx);

    // Query registry
    let registry = render_registry(client_diag, lab_diag, expanded_key, sort_by, status_filter, cx);

    div().gap_5().flex().flex_col().child(hero).child(overview).child(actions).child(registry)
}

// ---------------------------------------------------------------------------
// Stat Card
// ---------------------------------------------------------------------------

fn stat_card(label: &str, value: String, radius_lg: gpui::Pixels, border: Hsla, muted: Hsla, muted_foreground: Hsla) -> Div {
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

fn render_action_bar(cx: &mut Context<QueryDevToolsPage>) -> Div {
    // Extract theme colors upfront to avoid holding a borrow on cx across
    // the mutable borrow required by cx.listener().
    let theme = cx.theme();
    let radius_lg = theme.radius_lg;
    let border = theme.border;
    let muted = theme.muted;
    let _ = theme;

    let has_client = cx.has_global::<QueryClient>();

    let invalidate = Button::new("devtools-invalidate-all")
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

    let reset = Button::new("devtools-reset-all")
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

    let gc = Button::new("devtools-gc")
        .outline()
        .label("GC")
        .when(!has_client, |btn| btn.disabled(true))
        .on_click(cx.listener(|_, _, _, cx| {
            if cx.has_global::<QueryClient>() {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                cx.update_global::<QueryClient, _>(|client, cx| {
                    client.gc(cx, now_ms);
                });
                cx.notify();
            }
        }));

    let remove = Button::new("devtools-remove-all")
        .outline()
        .label("Remove All")
        .when(!has_client, |btn| btn.disabled(true))
        .on_click(cx.listener(|_, _, _, cx| {
            if cx.has_global::<QueryClient>() {
                cx.update_global::<QueryClient, _>(|client, cx| {
                    client.remove_queries(&QueryKeyFilter::All, cx);
                });
                cx.notify();
            }
        }));

    let clear = Button::new("devtools-clear")
        .outline()
        .label("Clear")
        .when(!has_client, |btn| btn.disabled(true))
        .on_click(cx.listener(|_, _, _, cx| {
            if cx.has_global::<QueryClient>() {
                cx.update_global::<QueryClient, _>(|client, _cx| {
                    client.clear();
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
            h_flex().gap_2().flex_wrap().children(vec![invalidate, reset, gc, remove, clear]),
        )
}

// ---------------------------------------------------------------------------
// Registry (sort/filter controls + table)
// ---------------------------------------------------------------------------

fn render_registry(
    client_diag: &Option<gpui_query::client::devtools::ClientDiagnostic>,
    lab_diag: &Option<Vec<HttpLabDiagnostic>>,
    expanded_key: &Option<String>,
    sort_by: QuerySort,
    status_filter: &Option<String>,
    cx: &mut Context<QueryDevToolsPage>,
) -> Div {
    // Extract theme colors upfront to avoid holding a borrow on cx.
    let radius_lg = cx.theme().radius_lg;
    let border = cx.theme().border;
    let muted = cx.theme().muted;
    let muted_foreground = cx.theme().muted_foreground;

    // Sort & filter controls
    let sort_controls = h_flex().gap_2().children(vec![
        sort_button("By Key", QuerySort::ByKey, sort_by, cx),
        sort_button("By Status", QuerySort::ByStatus, sort_by, cx),
        sort_button("By Cache Hits", QuerySort::ByCacheHits, sort_by, cx),
    ]);

    let status_options = [
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

    // Merge rows from both data sources into a unified display format.
    let mut rows: Vec<UnifiedRow> = Vec::new();

    if let Some(diag) = client_diag {
        for q in &diag.queries {
            rows.push(UnifiedRow {
                key: q.key.clone(),
                source: "QueryClient".to_string(),
                status: q.status.clone(),
                has_data: q.has_data,
                has_error: q.has_error,
                cache_policy: q.cache_policy.clone(),
                request_policy: q.request_policy.clone(),
                cache_hits: q.cache_hits,
            });
        }
    }

    if let Some(lab) = lab_diag {
        for d in lab {
            rows.push(UnifiedRow {
                key: format!("http_lab/{}", d.action),
                source: "HTTP Lab".to_string(),
                status: d.status.clone(),
                has_data: d.has_data,
                has_error: d.has_error,
                cache_policy: d.cache_policy.clone(),
                request_policy: d.request_policy.clone(),
                cache_hits: d.cache_hits,
            });
        }
    }

    // Apply sort + filter
    if let Some(filter) = status_filter {
        rows.retain(|r| r.status == *filter);
    }
    match sort_by {
        QuerySort::ByKey => rows.sort_by(|a, b| a.key.cmp(&b.key)),
        QuerySort::ByStatus => rows.sort_by(|a, b| a.status.cmp(&b.status)),
        QuerySort::ByCacheHits => rows.sort_by(|a, b| b.cache_hits.cmp(&a.cache_hits)),
    }

    // Build table
    let table_rows: Vec<_> = rows
        .iter()
        .map(|r| {
            let is_expanded = expanded_key.as_deref() == Some(r.key.as_str());
            let row = unified_row(r, is_expanded, cx).into_any_element();
            let detail = if is_expanded {
                Some(expanded_detail_for(r).into_any_element())
            } else {
                None
            };
            (row, detail)
        })
        .collect();

    let table = v_flex().gap_0p5().children(
        table_rows.into_iter().flat_map(|(row, detail)| {
            let mut children = vec![row];
            if let Some(d) = detail {
                children.push(d);
            }
            children
        }),
    );

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
                v_flex().gap_1()
                    .child(div().text_xs().text_color(muted_foreground).child("Sort:"))
                    .child(sort_controls)
            ).child(
                v_flex().gap_1()
                    .child(div().text_xs().text_color(muted_foreground).child("Status:"))
                    .child(filter_controls)
            ),
        )
        .child(table)
}

// ---------------------------------------------------------------------------
// Sort Button
// ---------------------------------------------------------------------------

fn sort_button(
    label: &str,
    target: QuerySort,
    current: QuerySort,
    cx: &mut Context<QueryDevToolsPage>,
) -> Button {
    let active = current == target;
    Button::new(format!("sort-{:?}", target))
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
    cx: &mut Context<QueryDevToolsPage>,
) -> Button {
    let label = target.unwrap_or("All");
    let active = match (current, target) {
        (None, None) => true,
        (Some(cur), Some(tgt)) => cur == tgt,
        _ => false,
    };
    let id = format!("filter-{}", target.unwrap_or("all"));
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
// Unified Row
// ---------------------------------------------------------------------------

struct UnifiedRow {
    key: String,
    source: String,
    status: String,
    has_data: bool,
    has_error: bool,
    cache_policy: String,
    request_policy: String,
    cache_hits: u64,
}

fn unified_row(
    r: &UnifiedRow,
    is_expanded: bool,
    cx: &mut Context<QueryDevToolsPage>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let muted_foreground = theme.muted_foreground;
    let primary = theme.primary;
    let danger = theme.danger;
    let radius = theme.radius;
    let secondary = theme.secondary;
    let _ = theme;

    let key = r.key.clone();

    let status_color = match r.status.as_str() {
        "Idle" | "Cancelled" => muted_foreground,
        "Success" | "LoadingEmpty" | "LoadingWithData" => primary,
        "Failure" => danger,
        _ => muted_foreground,
    };

    let data_dot = if r.has_data {
        div().text_xs().text_color(primary).child("[data]")
    } else {
        div()
    };

    let error_dot = if r.has_error {
        div().text_xs().text_color(danger).child("[error]")
    } else {
        div()
    };

    let chevron = if is_expanded { "▾" } else { "▸" };

    let source_label = div()
        .text_xs()
        .px_1()
        .rounded(radius)
        .bg(secondary)
        .child(r.source.clone());

    div()
        .id(SharedString::from(format!("query-row-{}", r.key)))
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
            h_flex().gap_3().items_center().children(vec![
                div().text_xs().text_color(muted_foreground).child(chevron.to_string()),
                source_label,
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(status_color)
                    .child(r.status.clone()),
                div().text_sm().font_family("monospace").child(r.key.clone()),
                data_dot,
                error_dot,
                div()
                    .text_xs()
                    .px_1()
                    .rounded(radius)
                    .bg(secondary)
                    .child(r.cache_policy.clone()),
                div()
                    .text_xs()
                    .text_color(muted_foreground)
                    .child(format!("hits:{}", r.cache_hits)),
            ]),
        )
}

// ---------------------------------------------------------------------------
// Expanded Detail (unified)
// ---------------------------------------------------------------------------

fn expanded_detail_for(r: &UnifiedRow) -> Div {
    let fields: Vec<(&str, String)> = vec![
        ("Key", r.key.clone()),
        ("Source", r.source.clone()),
        ("Status", r.status.clone()),
        ("Has Data", r.has_data.to_string()),
        ("Has Error", r.has_error.to_string()),
        ("Cache Policy", r.cache_policy.clone()),
        ("Request Policy", r.request_policy.clone()),
        ("Cache Hits", r.cache_hits.to_string()),
    ];

    div()
        .ml_4()
        .border_1()
        .p_3()
        .gap_1()
        .children(fields.into_iter().map(|(label, value)| {
            h_flex().gap_2().child(
                div().text_xs().child(
                    h_flex().gap_1()
                        .child(div().font_weight(FontWeight::BOLD).child(format!("{}:", label)))
                        .child(div().child(value)),
                ),
            )
        }))
}
