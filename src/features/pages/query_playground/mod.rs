//! Query V2 Playground — interactive demo of every gpui-query-v2 feature.
//!
//! Each section exercises a distinct capability: simple queries, cache policies,
//! request policies, retry, mutations (with callbacks), infinite queries,
//! select transforms, and imperative fetch with signal cancellation.

mod queries;
mod render_sections;
mod ui_helpers;

use std::sync::Arc;

use gpui::prelude::*;
use gpui::*;

use gpui_component::{
    ActiveTheme as _, VirtualListScrollHandle,
    input::{InputEvent, InputState},
    v_flex,
};
use serde::{Deserialize, Serialize};

use gpui_query::client::QueryClient;
use gpui_query::core::{
    InfiniteQueryResource, MappedQueryResource, MutationResource, QueryError, QueryResource,
};

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaygroundUser {
    pub id: u32,
    pub name: String,
    pub email: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaygroundPage {
    pub items: Vec<String>,
    pub page_number: u32,
}

/// A real httpbin response captured by the HTTP Fetching section, used to demo
/// gpui-query managing live network requests (reqwest over the tokio runtime).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HttpFetchResult {
    pub method: String,
    pub label: String,
    pub url: String,
    pub status: u16,
    pub content_type: String,
    pub body: String,
    pub elapsed_ms: u64,
}

/// Which httpbin request the HTTP Fetching section should perform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HttpFetchKind {
    GetJson,
    GetXml,
    GetText,
    PostJson,
    GetFail,
}

impl HttpFetchKind {
    pub(super) fn method(self) -> &'static str {
        match self {
            Self::GetJson | Self::GetXml | Self::GetText | Self::GetFail => "GET",
            Self::PostJson => "POST",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::GetJson => "GET JSON",
            Self::GetXml => "GET XML",
            Self::GetText => "GET text",
            Self::PostJson => "POST JSON",
            Self::GetFail => "GET fail",
        }
    }

    pub(super) fn url(self) -> &'static str {
        match self {
            Self::GetJson => "https://httpbin.org/json",
            Self::GetXml => "https://httpbin.org/xml",
            Self::GetText => "https://httpbin.org/encoding/utf8",
            Self::PostJson => "https://httpbin.org/post",
            Self::GetFail => "https://httpbin.org/status/500",
        }
    }

    pub(super) fn accept_header(self) -> &'static str {
        match self {
            Self::GetJson => "application/json",
            Self::GetXml => "application/xml",
            Self::GetText => "text/plain",
            Self::PostJson => "application/json",
            Self::GetFail => "*/*",
        }
    }
}

// ---------------------------------------------------------------------------
// Page struct
// ---------------------------------------------------------------------------

pub struct QueryPlaygroundPage {
    pub(super) _subscriptions: Vec<Subscription>,
    // Simple query
    pub(super) simple_query: Option<(
        Entity<QueryResource<PlaygroundUser, QueryError>>,
        Subscription,
    )>,
    // Cache policy demos
    pub(super) nocache_query: Option<(
        Entity<QueryResource<PlaygroundUser, QueryError>>,
        Subscription,
    )>,
    pub(super) ttl_query: Option<(
        Entity<QueryResource<PlaygroundUser, QueryError>>,
        Subscription,
    )>,
    pub(super) swr_query: Option<(
        Entity<QueryResource<PlaygroundUser, QueryError>>,
        Subscription,
    )>,
    // Request policy demos
    pub(super) latest_wins_query: Option<(Entity<QueryResource<String, QueryError>>, Subscription)>,
    pub(super) ignore_query: Option<(Entity<QueryResource<String, QueryError>>, Subscription)>,
    // Retry demo
    pub(super) retry_query: Option<(Entity<QueryResource<String, QueryError>>, Subscription)>,
    /// High-water mark of retry attempts for the retry demo. The crate resets
    /// `retry_count` to 0 on terminal failure, so we track the peak for display.
    pub(super) retry_peak: u32,
    // Mutation demo
    pub(super) mutation_entity: Option<(
        Entity<MutationResource<String, String, QueryError>>,
        Subscription,
    )>,
    // Mutation input state
    pub(super) mutation_input_state: Entity<InputState>,
    // Infinite query
    pub(super) infinite_entity: Option<(
        Entity<InfiniteQueryResource<PlaygroundPage, QueryError>>,
        Subscription,
    )>,
    // Select transform
    pub(super) select_source: Option<Entity<QueryResource<Vec<PlaygroundUser>, QueryError>>>,
    pub(super) select_mapped:
        Option<Entity<MappedQueryResource<Vec<PlaygroundUser>, Vec<String>, QueryError>>>,
    pub(super) _select_subs: Option<(Subscription, Subscription)>,
    // Imperative fetch
    pub(super) imperative_query: Option<(Entity<QueryResource<String, QueryError>>, Subscription)>,
    // Real HTTP fetch (reqwest via the tokio runtime)
    pub(super) http_query: Option<(
        Entity<QueryResource<HttpFetchResult, QueryError>>,
        Subscription,
    )>,
    // UI state
    pub(super) activity_log: Vec<String>,
    pub(super) log_scroll_handle: VirtualListScrollHandle,
    // Shared callback log that survives past method returns (Finding 2)
    pub(super) _callback_log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl QueryPlaygroundPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subs = Vec::new();
        subs.push(cx.observe_global_in::<QueryClient>(window, |_, _, cx| {
            cx.notify();
        }));

        // Finding 1/8: Create a proper editable InputState for mutation input.
        let mutation_input_state =
            cx.new(|cx| InputState::new(window, cx).placeholder("Enter mutation variables..."));

        // Subscribe to input changes so the UI re-reads from mutation_input_state.
        subs.push(cx.subscribe(
            &mutation_input_state,
            |_this: &mut Self, _state: Entity<InputState>, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    cx.notify();
                }
            },
        ));

        let callback_log = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

        Self {
            _subscriptions: subs,
            simple_query: None,
            nocache_query: None,
            ttl_query: None,
            swr_query: None,
            latest_wins_query: None,
            ignore_query: None,
            retry_query: None,
            retry_peak: 0,
            mutation_entity: None,
            mutation_input_state,
            infinite_entity: None,
            select_source: None,
            select_mapped: None,
            _select_subs: None,
            imperative_query: None,
            http_query: None,
            activity_log: Vec::new(),
            log_scroll_handle: VirtualListScrollHandle::new(),
            _callback_log: callback_log,
        }
    }

    /// Read the current mutation input text from the InputState entity.
    pub(super) fn mutation_input_value(&self, cx: &App) -> String {
        self.mutation_input_state.read(cx).value().to_string()
    }

    pub(super) fn log(&mut self, msg: impl Into<String>) {
        self.activity_log.push(msg.into());
        // Cap at 30 entries to limit DOM overhead.
        if self.activity_log.len() > 30 {
            self.activity_log.drain(0..self.activity_log.len() - 30);
        }
    }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

impl Render for QueryPlaygroundPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Extract theme colors upfront to release borrows before calling methods.
        let theme = cx.theme();
        let radius_lg = theme.radius_lg;
        let border = theme.border;
        let muted = theme.muted;
        let muted_foreground = theme.muted_foreground;
        let _ = theme;

        let page = v_flex()
            .id("query-playground-page")
            .min_h_full()
            .p_6()
            .gap_5()
            .overflow_y_scroll()
            // -- Header --
            .child(
                div()
                    .p_5()
                    .rounded(radius_lg)
                    .border_1()
                    .border_color(border)
                    .bg(muted)
                    .child(
                        v_flex()
                            .gap_3()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight::BOLD)
                                    .child("Query V2 Playground"),
                            )
                            .child(
                                div()
                                    .max_w(px(800.))
                                    .text_sm()
                                    .text_color(muted_foreground)
                                    .child(
                                        "Interactive demo of every gpui-query-v2 feature: queries, \
                                         cache policies, request policies, retry, mutations, \
                                         infinite queries, select transforms, and imperative fetch \
                                         with signal cancellation.",
                                    ),
                            ),
                    ),
            )
            // -- 1. Simple Query --
            .child(self.render_simple_query(cx))
            // -- 2. Cache Policies --
            .child(self.render_cache_policies(cx))
            // -- 3. Request Policies --
            .child(self.render_request_policies(cx))
            // -- 4. Retry Policy --
            .child(self.render_retry_policy(cx))
            // -- 5. Mutation --
            .child(self.render_mutation(cx))
            // -- 6. Infinite Query --
            .child(self.render_infinite_query(cx))
            // -- 7. Select Transform --
            .child(self.render_select_transform(cx))
            // -- 8. Imperative Fetch --
            .child(self.render_imperative_fetch(cx))
            // -- 9. HTTP Fetching (real network via reqwest) --
            .child(self.render_http_fetching(cx))
            // -- 10. Activity Log --
            .child(self.render_activity_log(cx));

        page
    }
}
