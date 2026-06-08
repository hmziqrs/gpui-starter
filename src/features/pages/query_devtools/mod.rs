use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _,
    Icon, IconName,
    h_flex, v_flex,
};
use gpui_query::client::QueryClient;

use crate::services::http_lab::{HttpLabDiagnostic, HttpLabState};

mod action_bar;
mod registry;
mod sort_types;

use sort_types::QuerySort;

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
    let actions = action_bar::render_action_bar(cx);

    // Query registry
    let registry = registry::render_registry(client_diag, lab_diag, expanded_key, sort_by, status_filter, cx);

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
