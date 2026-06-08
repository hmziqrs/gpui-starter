mod actions;
mod network;
mod render;
mod ui_helpers;

use std::collections::BTreeMap;

use gpui::{prelude::*, *};
use tokio_util::sync::CancellationToken;

use gpui_query::{CachePolicy, QueryResource, RequestPolicy, RequestSequencer};

use crate::services::http_lab::HttpLabAction;

pub(crate) const LOG: &str = "gpui_starter::http_lab_testing";
pub(crate) const RENDER_LOG: &str = "gpui_starter::http_lab_testing::render";
pub(crate) const TEST_URL: &str = "https://httpbin.org/get";
pub(crate) const PREVIEW_LIMIT: usize = 8_000;

#[derive(Clone, Debug)]
pub(crate) enum RawStatus {
    Idle,
    Sending,
    Completed,
    Failed,
    Cancelled,
}

impl RawStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Sending => "Sending",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawResponse {
    status: u16,
    final_url: String,
    header_count: usize,
    bytes: usize,
    preview: String,
}

pub struct HttpLabTestingPage {
    next_operation_id: u64,
    active_operation_id: Option<u64>,
    cancellation: Option<CancellationToken>,
    status: RawStatus,
    last_message: String,
    last_response: Option<RawResponse>,
    query_resource: QueryResource<RawResponse>,
    query_ttl_resource: QueryResource<RawResponse>,
    query_ignore_resource: QueryResource<RawResponse>,
    query_latest_resource: QueryResource<RawResponse>,
    query_sequencer: RequestSequencer,
    query_message: String,
    local_lab_resources: BTreeMap<HttpLabAction, QueryResource<RawResponse>>,
    local_lab_sequencer: RequestSequencer,
    local_lab_selected: HttpLabAction,
    local_lab_history: Vec<(HttpLabAction, RawResponse)>,
    local_lab_message: String,
    // Signal exercise
    query_signal_resource: QueryResource<RawResponse>,
    query_signal_sequencer: RequestSequencer,
    query_signal_message: String,
    // Placeholder / previous data exercise
    query_placeholder_resource: QueryResource<RawResponse>,
    query_placeholder_sequencer: RequestSequencer,
    query_placeholder_message: String,
    // Optimistic update exercise
    query_optimistic_resource: QueryResource<RawResponse>,
    query_optimistic_sequencer: RequestSequencer,
    query_optimistic_message: String,
    // Client fetch exercise
    client_query_message: String,
    show_query_details: bool,
    show_signal_details: bool,
    show_retention_details: bool,
    show_optimistic_details: bool,
    show_client_details: bool,
    show_local_history: bool,
    show_response_details: bool,
    show_response_preview: bool,
}

impl HttpLabTestingPage {
    pub fn new() -> Self {
        Self {
            next_operation_id: 1,
            active_operation_id: None,
            cancellation: None,
            status: RawStatus::Idle,
            last_message: "No request sent yet.".to_string(),
            last_response: None,
            query_resource: QueryResource::new(
                "http_lab_testing/raw_query",
                CachePolicy::NoCache,
                RequestPolicy::LatestWins,
            ),
            query_ttl_resource: QueryResource::new(
                "http_lab_testing/ttl_query",
                CachePolicy::Ttl { ttl_ms: 30_000 },
                RequestPolicy::LatestWins,
            ),
            query_ignore_resource: QueryResource::new(
                "http_lab_testing/ignore_query",
                CachePolicy::NoCache,
                RequestPolicy::IgnoreWhileLoading,
            ),
            query_latest_resource: QueryResource::new(
                "http_lab_testing/latest_query",
                CachePolicy::NoCache,
                RequestPolicy::LatestWins,
            ),
            query_sequencer: RequestSequencer::new(),
            query_message: "No query request sent yet.".to_string(),
            local_lab_resources: local_lab_resources(),
            local_lab_sequencer: RequestSequencer::new(),
            local_lab_selected: HttpLabAction::GetJson,
            local_lab_history: Vec::new(),
            local_lab_message: "No local full-query lab request sent yet.".to_string(),
            query_signal_resource: QueryResource::new(
                "http_lab_testing/signal_query",
                CachePolicy::NoCache,
                RequestPolicy::LatestWins,
            ),
            query_signal_sequencer: RequestSequencer::new(),
            query_signal_message: "No signal exercise run yet.".to_string(),
            query_placeholder_resource: QueryResource::new(
                "http_lab_testing/placeholder_query",
                CachePolicy::NoCache,
                RequestPolicy::LatestWins,
            ),
            query_placeholder_sequencer: RequestSequencer::new(),
            query_placeholder_message: "No placeholder exercise run yet.".to_string(),
            query_optimistic_resource: QueryResource::new(
                "http_lab_testing/optimistic_query",
                CachePolicy::NoCache,
                RequestPolicy::LatestWins,
            ),
            query_optimistic_sequencer: RequestSequencer::new(),
            query_optimistic_message: "No optimistic exercise run yet.".to_string(),
            client_query_message: "No client fetch exercise run yet.".to_string(),
            show_query_details: false,
            show_signal_details: false,
            show_retention_details: false,
            show_optimistic_details: false,
            show_client_details: false,
            show_local_history: false,
            show_response_details: false,
            show_response_preview: false,
        }
    }
}

pub(crate) fn query_now_ms() -> u128 {
    use std::sync::OnceLock;
    static STARTED_AT: OnceLock<std::time::Instant> = OnceLock::new();
    STARTED_AT.get_or_init(std::time::Instant::now).elapsed().as_millis()
}

pub(crate) fn fake_response(label: &str) -> RawResponse {
    RawResponse {
        status: 200,
        final_url: format!("memory://{label}"),
        header_count: 0,
        bytes: label.len(),
        preview: label.to_string(),
    }
}

pub(crate) fn local_lab_resources() -> BTreeMap<HttpLabAction, QueryResource<RawResponse>> {
    HttpLabAction::all()
        .iter()
        .copied()
        .map(|action| {
            (
                action,
                QueryResource::new(
                    format!("http_lab_testing/local/{}", action.id()),
                    local_lab_cache_policy(action),
                    local_lab_request_policy(action),
                ),
            )
        })
        .collect()
}

fn local_lab_cache_policy(action: HttpLabAction) -> CachePolicy {
    match action {
        HttpLabAction::GetText | HttpLabAction::GetXml => CachePolicy::Ttl { ttl_ms: 60_000 },
        HttpLabAction::GetJson => CachePolicy::StaleWhileRevalidate { ttl_ms: 30_000 },
        HttpLabAction::PostJson
        | HttpLabAction::PostForm
        | HttpLabAction::PostMultipart
        | HttpLabAction::Cookies
        | HttpLabAction::Failure
        | HttpLabAction::FullFlow => CachePolicy::NoCache,
    }
}

fn local_lab_request_policy(action: HttpLabAction) -> RequestPolicy {
    match action {
        HttpLabAction::PostMultipart | HttpLabAction::FullFlow => RequestPolicy::IgnoreWhileLoading,
        _ => RequestPolicy::LatestWins,
    }
}

