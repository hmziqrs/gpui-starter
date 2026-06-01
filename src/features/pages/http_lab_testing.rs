use std::{collections::BTreeMap, sync::OnceLock, time::Instant};

use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, Disableable as _,
    button::{Button, ButtonVariants as _},
    v_flex,
};
use tokio_util::sync::CancellationToken;

use gpui_query::{
    CachePolicy, QueryBeginResult, QueryError, QueryFetchMode, QueryResource, RequestPolicy,
    RequestSequencer,
};

use crate::services::{http_lab::HttpLabAction, tokio_runtime::TokioRuntimeGlobal};

const LOG: &str = "gpui_starter::http_lab_testing";
const RENDER_LOG: &str = "gpui_starter::http_lab_testing::render";
const TEST_URL: &str = "https://httpbin.org/get";
const PREVIEW_LIMIT: usize = 8_000;

#[derive(Clone, Debug)]
enum RawStatus {
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
struct RawResponse {
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

    fn send_raw_get(&mut self, cx: &mut Context<Self>) {
        let operation_id = self.next_operation_id;
        self.next_operation_id += 1;

        if let Some(token) = self.cancellation.take() {
            token.cancel();
        }

        let cancellation = CancellationToken::new();
        self.active_operation_id = Some(operation_id);
        self.cancellation = Some(cancellation.clone());
        self.status = RawStatus::Sending;
        self.last_message = format!("operation {operation_id}: dispatching raw GET");
        self.last_response = None;
        cx.notify();

        let runtime = cx.global::<TokioRuntimeGlobal>().0.runtime.clone();
        let client = cx.global::<TokioRuntimeGlobal>().0.http_client.clone();
        let url = TEST_URL.to_string();

        tracing::info!(
            target: LOG,
            operation_id,
            url,
            "HTTP Lab Testing scheduling entity foreground task"
        );

        cx.spawn(async move |this, cx| {
            tracing::info!(
                target: LOG,
                operation_id,
                "HTTP Lab Testing foreground task started"
            );

            let started = Instant::now();
            let request_cancellation = cancellation.clone();
            let handle = runtime.spawn(async move {
                raw_reqwest_get(client, url, request_cancellation, operation_id).await
            });

            tracing::info!(
                target: LOG,
                operation_id,
                "HTTP Lab Testing Tokio request task spawned"
            );

            let result = match handle.await {
                Ok(result) => result,
                Err(err) => Err(format!("tokio task join failed: {err}")),
            };

            let elapsed_ms = started.elapsed().as_millis();
            tracing::info!(
                target: LOG,
                operation_id,
                elapsed_ms,
                ok = result.is_ok(),
                "HTTP Lab Testing foreground task joined Tokio result"
            );

            this.update(cx, |this, cx| {
                if this.active_operation_id != Some(operation_id) {
                    tracing::info!(
                        target: LOG,
                        operation_id,
                        active_operation_id = ?this.active_operation_id,
                        "HTTP Lab Testing ignoring stale operation result"
                    );
                    return;
                }

                this.active_operation_id = None;
                this.cancellation = None;

                match result {
                    Ok(response) => {
                        this.status = RawStatus::Completed;
                        this.last_message =
                            format!("operation {operation_id}: completed in {elapsed_ms}ms");
                        this.last_response = Some(response);
                    }
                    Err(err) if err == "cancelled" => {
                        this.status = RawStatus::Cancelled;
                        this.last_message =
                            format!("operation {operation_id}: cancelled after {elapsed_ms}ms");
                        this.last_response = None;
                    }
                    Err(err) => {
                        this.status = RawStatus::Failed;
                        this.last_message =
                            format!("operation {operation_id}: failed after {elapsed_ms}ms: {err}");
                        this.last_response = None;
                    }
                }

                tracing::info!(
                    target: LOG,
                    operation_id,
                    status = this.status.label(),
                    "HTTP Lab Testing applying operation result"
                );
                cx.notify();
            })
            .ok();
        })
        .detach();

        tracing::info!(
            target: LOG,
            operation_id,
            "HTTP Lab Testing entity foreground task scheduled"
        );
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        let Some(operation_id) = self.active_operation_id.take() else {
            return;
        };

        if let Some(token) = self.cancellation.take() {
            token.cancel();
        }

        self.status = RawStatus::Cancelled;
        self.last_message = format!("operation {operation_id}: cancel requested");
        self.last_response = None;
        tracing::info!(
            target: LOG,
            operation_id,
            "HTTP Lab Testing cancellation requested"
        );
        cx.notify();
    }

    fn verdict(label: &str, passed: bool, detail: &str) -> String {
        let icon = if passed { "✅" } else { "❌" };
        format!("{icon} {label}  — {detail}")
    }

    fn send_query_get(&mut self, cx: &mut Context<Self>) {
        let operation_id = self.next_operation_id;
        self.next_operation_id += 1;

        if let Some(token) = self.cancellation.take() {
            token.cancel();
        }

        let query_started_ms = query_now_ms();
        tracing::info!(
            target: LOG,
            operation_id,
            query_started_ms,
            "HTTP Lab Testing query begin_request entered"
        );
        let request_id = match self.query_resource.begin_request(
            &mut self.query_sequencer,
            query_started_ms,
            QueryFetchMode::Normal,
        ) {
            QueryBeginResult::Started {
                request_id,
                status,
                replaced_request_id,
            } => {
                tracing::info!(
                    target: LOG,
                    operation_id,
                    request_id = %request_id.label(),
                    status = status.label(),
                    replaced_request_id = ?replaced_request_id.map(|id| id.label()),
                    "HTTP Lab Testing query request started"
                );
                request_id
            }
            QueryBeginResult::CacheHit => {
                self.query_message = "query cache hit".to_string();
                tracing::info!(
                    target: LOG,
                    operation_id,
                    "HTTP Lab Testing query cache hit"
                );
                cx.notify();
                return;
            }
            QueryBeginResult::IgnoredWhileLoading { active_request_id } => {
                self.query_message =
                    format!("query ignored while loading {}", active_request_id.label());
                tracing::info!(
                    target: LOG,
                    operation_id,
                    active_request_id = %active_request_id.label(),
                    "HTTP Lab Testing query ignored while loading"
                );
                cx.notify();
                return;
            }
        };

        let cancellation = CancellationToken::new();
        self.active_operation_id = Some(operation_id);
        self.cancellation = Some(cancellation.clone());
        self.status = RawStatus::Sending;
        self.last_message = format!("operation {operation_id}: dispatching query GET");
        self.query_message = format!(
            "operation {operation_id}: query request {} loading",
            request_id.label()
        );
        self.last_response = None;
        cx.notify();

        let runtime = cx.global::<TokioRuntimeGlobal>().0.runtime.clone();
        let client = cx.global::<TokioRuntimeGlobal>().0.http_client.clone();
        let url = TEST_URL.to_string();

        tracing::info!(
            target: LOG,
            operation_id,
            request_id = %request_id.label(),
            url,
            "HTTP Lab Testing scheduling query foreground task"
        );

        cx.spawn(async move |this, cx| {
            tracing::info!(
                target: LOG,
                operation_id,
                request_id = %request_id.label(),
                "HTTP Lab Testing query foreground task started"
            );

            let started = Instant::now();
            let request_cancellation = cancellation.clone();
            let handle = runtime.spawn(async move {
                raw_reqwest_get(client, url, request_cancellation, operation_id).await
            });

            tracing::info!(
                target: LOG,
                operation_id,
                request_id = %request_id.label(),
                "HTTP Lab Testing query Tokio request task spawned"
            );

            let result = match handle.await {
                Ok(result) => result,
                Err(err) => Err(format!("tokio task join failed: {err}")),
            };

            let elapsed_ms = started.elapsed().as_millis();
            tracing::info!(
                target: LOG,
                operation_id,
                request_id = %request_id.label(),
                elapsed_ms,
                ok = result.is_ok(),
                "HTTP Lab Testing query foreground task joined Tokio result"
            );

            if let Err(err) = this.update(cx, |this, cx| {
                if this.active_operation_id != Some(operation_id) {
                    tracing::info!(
                        target: LOG,
                        operation_id,
                        request_id = %request_id.label(),
                        active_operation_id = ?this.active_operation_id,
                        "HTTP Lab Testing query ignoring stale operation result"
                    );
                    return;
                }

                this.active_operation_id = None;
                this.cancellation = None;

                match result {
                    Ok(response) => {
                        let completed = this.query_resource.complete_current_success(
                            request_id,
                            response.clone(),
                            query_now_ms(),
                        );
                        this.status = RawStatus::Completed;
                        this.last_message =
                            format!("operation {operation_id}: query completed in {elapsed_ms}ms");
                        this.query_message = format!(
                            "operation {operation_id}: query complete accepted={completed}"
                        );
                        this.last_response = Some(response);
                    }
                    Err(err) if err == "cancelled" => {
                        let cancelled = this
                            .query_resource
                            .complete_current_failure(request_id, QueryError::cancelled(err));
                        this.status = RawStatus::Cancelled;
                        this.last_message =
                            format!("operation {operation_id}: query cancelled after {elapsed_ms}ms");
                        this.query_message = format!(
                            "operation {operation_id}: query cancel accepted={cancelled}"
                        );
                        this.last_response = None;
                    }
                    Err(err) => {
                        let completed = this
                            .query_resource
                            .complete_current_failure(request_id, QueryError::transport(err.clone()));
                        this.status = RawStatus::Failed;
                        this.last_message =
                            format!("operation {operation_id}: query failed after {elapsed_ms}ms: {err}");
                        this.query_message = format!(
                            "operation {operation_id}: query failure accepted={completed}"
                        );
                        this.last_response = None;
                    }
                }

                tracing::info!(
                    target: LOG,
                    operation_id,
                    request_id = %request_id.label(),
                    status = this.query_resource.status().label(),
                    active_request_id = ?this.query_resource.active_request_id().map(|id| id.label()),
                    "HTTP Lab Testing applying query result"
                );
                cx.notify();
            }) {
                tracing::warn!(
                    target: LOG,
                    operation_id,
                    request_id = %request_id.label(),
                    error = %err,
                    "HTTP Lab Testing failed to apply query result"
                );
            }
        })
        .detach();

        tracing::info!(
            target: LOG,
            operation_id,
            request_id = %request_id.label(),
            "HTTP Lab Testing query foreground task scheduled"
        );
    }

    fn exercise_query_ttl_cache(&mut self, cx: &mut Context<Self>) {
        let started_ms = query_now_ms();
        let first = self.query_ttl_resource.begin_request(
            &mut self.query_sequencer,
            started_ms,
            QueryFetchMode::Normal,
        );

        let QueryBeginResult::Started { request_id, .. } = first else {
            self.query_message = format!("TTL setup did not start: {first:?}");
            cx.notify();
            return;
        };

        let accepted = self.query_ttl_resource.complete_current_success(
            request_id,
            fake_response("ttl-cache"),
            started_ms + 1,
        );
        let second = self.query_ttl_resource.begin_request(
            &mut self.query_sequencer,
            started_ms + 2,
            QueryFetchMode::Normal,
        );
        let cache_hit = matches!(second, QueryBeginResult::CacheHit);
        let v_accepted = Self::verdict("first request started", accepted, &format!("accepted={accepted}"));
        let v_cache_hit = Self::verdict("cache hit on retry", cache_hit, &format!("cache_hit={cache_hit}"));
        let all_passed = accepted && cache_hit;
        let verdict_line = if all_passed { "TTL cache probe PASSED" } else { "TTL cache probe FAILED" };
        self.query_message = format!("{v_accepted}\n{v_cache_hit}\n{verdict_line}");

        tracing::info!(
            target: LOG,
            request_id = %request_id.label(),
            accepted,
            cache_hit,
            status = self.query_ttl_resource.status().label(),
            "HTTP Lab Testing TTL query cache probe completed"
        );
        cx.notify();
    }

    fn exercise_query_ignore_while_loading(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();
        let first = self.query_ignore_resource.begin_request(
            &mut self.query_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );

        let QueryBeginResult::Started { request_id, .. } = first else {
            self.query_message = format!("Ignore setup did not start: {first:?}");
            cx.notify();
            return;
        };

        let second = self.query_ignore_resource.begin_request(
            &mut self.query_sequencer,
            now_ms + 1,
            QueryFetchMode::Normal,
        );
        let ignored = matches!(
            second,
            QueryBeginResult::IgnoredWhileLoading { active_request_id }
                if active_request_id == request_id
        );
        let cancelled = self
            .query_ignore_resource
            .cancel(QueryError::cancelled("ignore probe cleanup"));
        let v_ignored = Self::verdict("duplicate ignored", ignored, &format!("ignored={ignored}"));
        let v_cancelled = Self::verdict("cleanup cancelled", cancelled, &format!("cancelled={cancelled}"));
        let all_passed = ignored && cancelled;
        let verdict_line = if all_passed { "Ignore-while-loading probe PASSED" } else { "Ignore-while-loading probe FAILED" };
        self.query_message = format!("{v_ignored}\n{v_cancelled}\n{verdict_line}");

        tracing::info!(
            target: LOG,
            request_id = %request_id.label(),
            ignored,
            cancelled,
            status = self.query_ignore_resource.status().label(),
            "HTTP Lab Testing ignore-while-loading query probe completed"
        );
        cx.notify();
    }

    fn exercise_query_latest_wins(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();
        let first = self.query_latest_resource.begin_request(
            &mut self.query_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );
        let QueryBeginResult::Started {
            request_id: first_id,
            ..
        } = first
        else {
            self.query_message = format!("Latest setup did not start: {first:?}");
            cx.notify();
            return;
        };

        let second = self.query_latest_resource.begin_request(
            &mut self.query_sequencer,
            now_ms + 1,
            QueryFetchMode::Normal,
        );
        let QueryBeginResult::Started {
            request_id: second_id,
            replaced_request_id,
            ..
        } = second
        else {
            self.query_message = format!("Latest replacement did not start: {second:?}");
            cx.notify();
            return;
        };

        let stale_accepted = self.query_latest_resource.complete_current_success(
            first_id,
            fake_response("latest-stale"),
            now_ms + 2,
        );
        let latest_accepted = self.query_latest_resource.complete_current_success(
            second_id,
            fake_response("latest-current"),
            now_ms + 3,
        );
        let replaced = replaced_request_id.is_some();
        let v_replaced = Self::verdict("second replaced first", replaced, &format!("replaced={:?}", replaced_request_id.map(|id| id.label())));
        let v_stale = Self::verdict("stale rejected", !stale_accepted, &format!("stale_accepted={stale_accepted}"));
        let v_latest = Self::verdict("latest accepted", latest_accepted, &format!("latest_accepted={latest_accepted}"));
        let all_passed = replaced && !stale_accepted && latest_accepted;
        let verdict_line = if all_passed { "Latest-wins probe PASSED" } else { "Latest-wins probe FAILED" };
        self.query_message = format!("{v_replaced}\n{v_stale}\n{v_latest}\n{verdict_line}");

        tracing::info!(
            target: LOG,
            first_id = %first_id.label(),
            second_id = %second_id.label(),
            replaced_request_id = ?replaced_request_id.map(|id| id.label()),
            stale_accepted,
            latest_accepted,
            status = self.query_latest_resource.status().label(),
            "HTTP Lab Testing latest-wins query probe completed"
        );
        cx.notify();
    }

    fn reset_local_lab(&mut self, cx: &mut Context<Self>) {
        if let Some(token) = self.cancellation.take() {
            token.cancel();
        }
        self.active_operation_id = None;
        self.local_lab_resources = local_lab_resources();
        self.local_lab_sequencer.advance_scope();
        self.local_lab_history.clear();
        self.local_lab_message = "Local full-query lab reset.".to_string();
        tracing::info!(target: LOG, "HTTP Lab Testing local full-query lab reset");
        cx.notify();
    }

    fn send_local_lab_action(&mut self, action: HttpLabAction, cx: &mut Context<Self>) {
        let operation_id = self.next_operation_id;
        self.next_operation_id += 1;

        if let Some(token) = self.cancellation.take() {
            token.cancel();
        }

        let now_ms = query_now_ms();
        self.local_lab_selected = action;
        if action == HttpLabAction::FullFlow {
            self.cancel_local_lab_active_requests("cancelled by local full flow");
        } else {
            self.cancel_local_lab_action(HttpLabAction::FullFlow, "cancelled by local request");
        }

        let request_id = match self
            .local_lab_resources
            .get_mut(&action)
            .expect("local lab resource must exist")
            .begin_request(
                &mut self.local_lab_sequencer,
                now_ms,
                QueryFetchMode::Normal,
            ) {
            QueryBeginResult::Started {
                request_id,
                status,
                replaced_request_id,
            } => {
                tracing::info!(
                    target: LOG,
                    operation_id,
                    action = action.id(),
                    request_id = %request_id.label(),
                    status = status.label(),
                    replaced_request_id = ?replaced_request_id.map(|id| id.label()),
                    "HTTP Lab Testing local lab query started"
                );
                request_id
            }
            QueryBeginResult::CacheHit => {
                self.local_lab_message = format!("{} local cache hit", action.label());
                tracing::info!(
                    target: LOG,
                    operation_id,
                    action = action.id(),
                    "HTTP Lab Testing local lab cache hit"
                );
                cx.notify();
                return;
            }
            QueryBeginResult::IgnoredWhileLoading { active_request_id } => {
                self.local_lab_message = format!(
                    "{} ignored while loading {}",
                    action.label(),
                    active_request_id.label()
                );
                tracing::info!(
                    target: LOG,
                    operation_id,
                    action = action.id(),
                    active_request_id = %active_request_id.label(),
                    "HTTP Lab Testing local lab ignored while loading"
                );
                cx.notify();
                return;
            }
        };

        let cancellation = CancellationToken::new();
        self.active_operation_id = Some(operation_id);
        self.cancellation = Some(cancellation.clone());
        self.status = RawStatus::Sending;
        self.last_message = format!("operation {operation_id}: local lab {}", action.label());
        self.local_lab_message = format!(
            "operation {operation_id}: {} loading request {}",
            action.label(),
            request_id.label()
        );
        self.last_response = None;
        cx.notify();

        let runtime = cx.global::<TokioRuntimeGlobal>().0.runtime.clone();
        let client = cx.global::<TokioRuntimeGlobal>().0.http_client.clone();

        tracing::info!(
            target: LOG,
            operation_id,
            action = action.id(),
            request_id = %request_id.label(),
            "HTTP Lab Testing scheduling local lab foreground task"
        );

        cx.spawn(async move |this, cx| {
            tracing::info!(
                target: LOG,
                operation_id,
                action = action.id(),
                request_id = %request_id.label(),
                "HTTP Lab Testing local lab foreground task started"
            );

            let started = Instant::now();
            let request_cancellation = cancellation.clone();
            let handle = runtime.spawn(async move {
                run_local_lab_action(client, action, request_cancellation, operation_id).await
            });

            tracing::info!(
                target: LOG,
                operation_id,
                action = action.id(),
                request_id = %request_id.label(),
                "HTTP Lab Testing local lab Tokio task spawned"
            );

            let result = match handle.await {
                Ok(result) => result,
                Err(err) => Err(format!("tokio task join failed: {err}")),
            };
            let elapsed_ms = started.elapsed().as_millis();

            tracing::info!(
                target: LOG,
                operation_id,
                action = action.id(),
                request_id = %request_id.label(),
                elapsed_ms,
                ok = result.is_ok(),
                "HTTP Lab Testing local lab foreground task joined Tokio result"
            );

            if let Err(err) = this.update(cx, |this, cx| {
                if this.active_operation_id != Some(operation_id) {
                    tracing::info!(
                        target: LOG,
                        operation_id,
                        action = action.id(),
                        request_id = %request_id.label(),
                        active_operation_id = ?this.active_operation_id,
                        "HTTP Lab Testing local lab ignored stale operation result"
                    );
                    return;
                }

                this.active_operation_id = None;
                this.cancellation = None;

                match result {
                    Ok(exchanges) => {
                        let mut accepted_count = 0usize;
                        let last_response = exchanges.last().map(|(_, response)| response.clone());
                        for (target_action, response) in exchanges {
                            if action == HttpLabAction::FullFlow
                                && target_action != HttpLabAction::FullFlow
                            {
                                this.complete_local_lab_child_resource(
                                    target_action,
                                    response.clone(),
                                );
                                accepted_count += 1;
                            } else {
                                let accepted = this
                                    .local_lab_resources
                                    .get_mut(&target_action)
                                    .expect("local lab resource must exist")
                                    .complete_current_success(
                                        request_id,
                                        response.clone(),
                                        query_now_ms(),
                                    );
                                if accepted {
                                    accepted_count += 1;
                                }
                            }
                            this.push_local_lab_history(target_action, response);
                        }

                        if action == HttpLabAction::FullFlow {
                            if let Some(response) = last_response.clone() {
                                let accepted = this
                                    .local_lab_resources
                                    .get_mut(&HttpLabAction::FullFlow)
                                    .expect("local lab full flow resource must exist")
                                    .complete_current_success(request_id, response, query_now_ms());
                                if accepted {
                                    accepted_count += 1;
                                }
                            }
                        }

                        this.status = RawStatus::Completed;
                        this.last_response = last_response;
                        this.last_message = format!(
                            "operation {operation_id}: local lab completed in {elapsed_ms}ms"
                        );
                        this.local_lab_message = format!(
                            "operation {operation_id}: {} accepted {accepted_count} updates",
                            action.label()
                        );
                    }
                    Err(err) if err == "cancelled" => {
                        let accepted = this
                            .local_lab_resources
                            .get_mut(&action)
                            .expect("local lab resource must exist")
                            .complete_current_failure(
                                request_id,
                                QueryError::cancelled("local lab cancelled"),
                            );
                        this.status = RawStatus::Cancelled;
                        this.last_message = format!(
                            "operation {operation_id}: local lab cancelled in {elapsed_ms}ms"
                        );
                        this.local_lab_message = format!(
                            "operation {operation_id}: {} cancel accepted={accepted}",
                            action.label()
                        );
                    }
                    Err(err) => {
                        let accepted = this
                            .local_lab_resources
                            .get_mut(&action)
                            .expect("local lab resource must exist")
                            .complete_current_failure(
                                request_id,
                                QueryError::transport(err.clone()),
                            );
                        this.status = RawStatus::Failed;
                        this.last_message =
                            format!("operation {operation_id}: local lab failed in {elapsed_ms}ms");
                        this.local_lab_message = format!(
                            "operation {operation_id}: {} failure accepted={accepted}: {err}",
                            action.label()
                        );
                    }
                }

                tracing::info!(
                    target: LOG,
                    operation_id,
                    action = action.id(),
                    request_id = %request_id.label(),
                    status = this.local_lab_resources[&action].status().label(),
                    history_len = this.local_lab_history.len(),
                    "HTTP Lab Testing local lab applied result"
                );
                cx.notify();
            }) {
                tracing::warn!(
                    target: LOG,
                    operation_id,
                    action = action.id(),
                    request_id = %request_id.label(),
                    error = %err,
                    "HTTP Lab Testing failed to apply local lab result"
                );
            }
        })
        .detach();

        tracing::info!(
            target: LOG,
            operation_id,
            action = action.id(),
            request_id = %request_id.label(),
            "HTTP Lab Testing local lab foreground task scheduled"
        );
    }

    fn cancel_local_lab_action(&mut self, action: HttpLabAction, reason: &str) {
        if let Some(resource) = self.local_lab_resources.get_mut(&action) {
            resource.cancel(QueryError::cancelled(reason));
        }
    }

    fn cancel_local_lab_active_requests(&mut self, reason: &str) {
        for action in HttpLabAction::all() {
            self.cancel_local_lab_action(*action, reason);
        }
    }

    fn complete_local_lab_child_resource(&mut self, action: HttpLabAction, response: RawResponse) {
        let now_ms = query_now_ms();
        let request_id = match self
            .local_lab_resources
            .get_mut(&action)
            .expect("local lab child resource must exist")
            .begin_request(&mut self.local_lab_sequencer, now_ms, QueryFetchMode::Force)
        {
            QueryBeginResult::Started { request_id, .. } => request_id,
            QueryBeginResult::CacheHit | QueryBeginResult::IgnoredWhileLoading { .. } => return,
        };
        self.local_lab_resources
            .get_mut(&action)
            .expect("local lab child resource must exist")
            .complete_current_success(request_id, response, now_ms + 1);
    }

    fn push_local_lab_history(&mut self, action: HttpLabAction, response: RawResponse) {
        self.local_lab_history.insert(0, (action, response));
        self.local_lab_history.truncate(16);
    }

    // -- Feature 1: Cancel Signal --

    fn exercise_query_signal(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();
        // Reset to clear any previous state.
        self.query_signal_resource.reset();
        let result = self.query_signal_resource.begin_request(
            &mut self.query_signal_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );

        let QueryBeginResult::Started { request_id: _, .. } = result else {
            self.query_signal_message = format!("Signal setup did not start: {result:?}");
            cx.notify();
            return;
        };

        // Clone the signal before cancelling.
        let signal = self.query_signal_resource.signal().cloned();
        let signal_present = signal.is_some();
        let before_cancel = signal.as_ref().map(|s| s.is_cancelled());

        // Cancel the resource — this should propagate to the signal.
        self.query_signal_resource
            .cancel(QueryError::cancelled("signal test"));
        let after_cancel = signal.as_ref().map(|s| s.is_cancelled());

        let v_signal = Self::verdict("signal present", signal_present, &format!("signal_present={signal_present}"));
        let before_ok = before_cancel == Some(false);
        let v_before = Self::verdict("signal active before cancel", before_ok, &format!("before_cancel={:?}", before_cancel));
        let after_ok = after_cancel == Some(true);
        let v_after = Self::verdict("signal cancelled after resource cancel", after_ok, &format!("after_cancel={:?}", after_cancel));
        let all_passed = signal_present && before_ok && after_ok;
        let verdict_line = if all_passed { "Cancel signal probe PASSED" } else { "Cancel signal probe FAILED" };
        self.query_signal_message = format!("{v_signal}\n{v_before}\n{v_after}\n{verdict_line}");
        cx.notify();
    }

    // -- Feature 3: Placeholder / Previous Data --

    fn exercise_query_placeholder_data(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();

        // Step 1: Seed the resource with real data.
        self.query_placeholder_resource.reset();
        let first = self.query_placeholder_resource.begin_request(
            &mut self.query_placeholder_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );
        let QueryBeginResult::Started {
            request_id: first_id,
            ..
        } = first
        else {
            self.query_placeholder_message = format!("Placeholder setup did not start: {first:?}");
            cx.notify();
            return;
        };
        self.query_placeholder_resource.complete_current_success(
            first_id,
            fake_response("original"),
            now_ms + 1,
        );

        // Step 2: Set placeholder data, then reset (clears data).
        self.query_placeholder_resource
            .set_placeholder_data(Some(fake_response("placeholder")));

        // Step 3: Reset clears data but NOT placeholder (actually reset DOES clear placeholder).
        // So set placeholder AFTER reset.
        self.query_placeholder_resource.reset();
        self.query_placeholder_resource
            .set_placeholder_data(Some(fake_response("placeholder")));

        // Step 4: Begin new request — during loading, display_data returns placeholder.
        let second = self.query_placeholder_resource.begin_request(
            &mut self.query_placeholder_sequencer,
            now_ms + 10,
            QueryFetchMode::Normal,
        );
        let loading_display = self
            .query_placeholder_resource
            .display_data()
            .map(|r| r.preview.clone());

        // Step 5: Complete with real data.
        if let QueryBeginResult::Started {
            request_id: second_id,
            ..
        } = second
        {
            self.query_placeholder_resource.complete_current_success(
                second_id,
                fake_response("real"),
                now_ms + 11,
            );
        }

        let final_display = self
            .query_placeholder_resource
            .display_data()
            .map(|r| r.preview.clone());
        let previous = self
            .query_placeholder_resource
            .previous_data()
            .map(|r| r.preview.clone());

        let loading_ok = loading_display.as_deref() == Some("placeholder");
        let v_loading = Self::verdict("placeholder shown during loading", loading_ok, &format!("loading_display={loading_display:?}"));
        let final_ok = final_display.as_deref() == Some("real");
        let v_final = Self::verdict("real data after completion", final_ok, &format!("final_display={final_display:?}"));
        let previous_ok = previous.as_deref() == Some("original");
        let v_previous = Self::verdict("previous tracked as original", previous_ok, &format!("previous={previous:?}"));
        let all_passed = loading_ok && final_ok && previous_ok;
        let verdict_line = if all_passed { "Placeholder data probe PASSED" } else { "Placeholder data probe FAILED" };
        self.query_placeholder_message = format!("{v_loading}\n{v_final}\n{v_previous}\n{verdict_line}");
        cx.notify();
    }

    fn exercise_query_previous_data(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();

        // Seed "first" then "second".
        self.query_placeholder_resource.reset();
        let first = self.query_placeholder_resource.begin_request(
            &mut self.query_placeholder_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );
        if let QueryBeginResult::Started { request_id, .. } = first {
            self.query_placeholder_resource.complete_current_success(
                request_id,
                fake_response("first"),
                now_ms + 1,
            );
        }

        let second = self.query_placeholder_resource.begin_request(
            &mut self.query_placeholder_sequencer,
            now_ms + 10,
            QueryFetchMode::Normal,
        );
        if let QueryBeginResult::Started { request_id, .. } = second {
            self.query_placeholder_resource.complete_current_success(
                request_id,
                fake_response("second"),
                now_ms + 11,
            );
        }

        let data = self
            .query_placeholder_resource
            .data()
            .map(|r| r.preview.clone());
        let previous = self
            .query_placeholder_resource
            .previous_data()
            .map(|r| r.preview.clone());

        let data_ok = data.as_deref() == Some("second");
        let v_data = Self::verdict("current data is 'second'", data_ok, &format!("data={data:?}"));
        let previous_ok = previous.as_deref() == Some("first");
        let v_previous = Self::verdict("previous data is 'first'", previous_ok, &format!("previous={previous:?}"));
        let all_passed = data_ok && previous_ok;
        let verdict_line = if all_passed { "Previous data probe PASSED" } else { "Previous data probe FAILED" };
        self.query_placeholder_message = format!("{v_data}\n{v_previous}\n{verdict_line}");
        cx.notify();
    }

    fn exercise_query_rollback(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();

        // Seed data, overwrite, then rollback.
        self.query_placeholder_resource.reset();
        let first = self.query_placeholder_resource.begin_request(
            &mut self.query_placeholder_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );
        if let QueryBeginResult::Started { request_id, .. } = first {
            self.query_placeholder_resource.complete_current_success(
                request_id,
                fake_response("original"),
                now_ms + 1,
            );
        }

        // Overwrite with new data.
        self.query_placeholder_resource
            .set_data(fake_response("overwritten"));

        // Rollback to previous.
        let rolled_back = self.query_placeholder_resource.rollback_to_previous();

        let data = self
            .query_placeholder_resource
            .data()
            .map(|r| r.preview.clone());

        let data_ok = data.as_deref() == Some("original");
        let v_rollback = Self::verdict("rollback succeeded", rolled_back, &format!("rolled_back={rolled_back}"));
        let v_data = Self::verdict("data restored to 'original'", data_ok, &format!("data={data:?}"));
        let all_passed = rolled_back && data_ok;
        let verdict_line = if all_passed { "Rollback probe PASSED" } else { "Rollback probe FAILED" };
        self.query_placeholder_message = format!("{v_rollback}\n{v_data}\n{verdict_line}");
        cx.notify();
    }

    // -- Feature 4: Optimistic Updates --

    fn exercise_query_optimistic_set(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();

        // Seed original data.
        self.query_optimistic_resource.reset();
        let first = self.query_optimistic_resource.begin_request(
            &mut self.query_optimistic_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );
        if let QueryBeginResult::Started { request_id, .. } = first {
            self.query_optimistic_resource.complete_current_success(
                request_id,
                fake_response("original"),
                now_ms + 1,
            );
        }

        // Optimistic update.
        self.query_optimistic_resource
            .set_data(fake_response("optimistic"));

        let data = self
            .query_optimistic_resource
            .data()
            .map(|r| r.preview.clone());
        let previous = self
            .query_optimistic_resource
            .previous_data()
            .map(|r| r.preview.clone());
        let status = self.query_optimistic_resource.status().label().to_string();

        let data_ok = data.as_deref() == Some("optimistic");
        let previous_ok = previous.as_deref() == Some("original");
        let status_ok = status == "Success";
        let v_data = Self::verdict("data is 'optimistic'", data_ok, &format!("data={data:?}"));
        let v_previous = Self::verdict("previous is 'original'", previous_ok, &format!("previous={previous:?}"));
        let v_status = Self::verdict("status is Success", status_ok, &format!("status={status}"));
        let all_passed = data_ok && previous_ok && status_ok;
        let verdict_line = if all_passed { "Optimistic set probe PASSED" } else { "Optimistic set probe FAILED" };
        self.query_optimistic_message = format!("{v_data}\n{v_previous}\n{v_status}\n{verdict_line}");
        cx.notify();
    }

    fn exercise_query_optimistic_rollback(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();

        // Seed original data.
        self.query_optimistic_resource.reset();
        let first = self.query_optimistic_resource.begin_request(
            &mut self.query_optimistic_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );
        if let QueryBeginResult::Started { request_id, .. } = first {
            self.query_optimistic_resource.complete_current_success(
                request_id,
                fake_response("original"),
                now_ms + 1,
            );
        }

        // Optimistic update then rollback.
        self.query_optimistic_resource
            .set_data(fake_response("optimistic"));
        let rolled_back = self.query_optimistic_resource.rollback_to_previous();

        let data = self
            .query_optimistic_resource
            .data()
            .map(|r| r.preview.clone());
        let data_ok = data.as_deref() == Some("original");
        let v_rollback = Self::verdict("rollback succeeded", rolled_back, &format!("rolled_back={rolled_back}"));
        let v_data = Self::verdict("data restored to 'original'", data_ok, &format!("data={data:?}"));
        let all_passed = rolled_back && data_ok;
        let verdict_line = if all_passed { "Optimistic rollback probe PASSED" } else { "Optimistic rollback probe FAILED" };
        self.query_optimistic_message = format!("{v_rollback}\n{v_data}\n{verdict_line}");
        cx.notify();
    }

    fn exercise_query_optimistic_flow(&mut self, cx: &mut Context<Self>) {
        let now_ms = query_now_ms();

        // Seed original.
        self.query_optimistic_resource.reset();
        let first = self.query_optimistic_resource.begin_request(
            &mut self.query_optimistic_sequencer,
            now_ms,
            QueryFetchMode::Normal,
        );
        if let QueryBeginResult::Started { request_id, .. } = first {
            self.query_optimistic_resource.complete_current_success(
                request_id,
                fake_response("original"),
                now_ms + 1,
            );
        }

        // Optimistic update.
        self.query_optimistic_resource
            .set_data(fake_response("optimistic"));

        // Simulate mutation success — begin request and complete with server data.
        let mutation = self.query_optimistic_resource.begin_request(
            &mut self.query_optimistic_sequencer,
            now_ms + 10,
            QueryFetchMode::Normal,
        );
        if let QueryBeginResult::Started { request_id, .. } = mutation {
            self.query_optimistic_resource.complete_current_success(
                request_id,
                fake_response("server confirmed"),
                now_ms + 11,
            );
        }

        let data = self
            .query_optimistic_resource
            .data()
            .map(|r| r.preview.clone());
        let previous = self
            .query_optimistic_resource
            .previous_data()
            .map(|r| r.preview.clone());

        let data_ok = data.as_deref() == Some("server confirmed");
        let previous_ok = previous.as_deref() == Some("optimistic");
        let v_data = Self::verdict("data is 'server confirmed'", data_ok, &format!("data={data:?}"));
        let v_previous = Self::verdict("previous is 'optimistic'", previous_ok, &format!("previous={previous:?}"));
        let all_passed = data_ok && previous_ok;
        let verdict_line = if all_passed { "Optimistic flow probe PASSED" } else { "Optimistic flow probe FAILED" };
        self.query_optimistic_message = format!("{v_data}\n{v_previous}\n{verdict_line}");
        cx.notify();
    }

    // -- Feature 2: Client fetchQuery --

    fn exercise_client_fetch_query(&mut self, cx: &mut Context<Self>) {
        let key = gpui_query::QueryKey::from_single("http_lab_testing/client_fetch");
        let now_ms = query_now_ms();

        if !cx.has_global::<gpui_query::client::QueryClient>() {
            cx.set_global(gpui_query::client::QueryClient::new(
                gpui_query::CachePolicy::default(),
                gpui_query::RequestPolicy::default(),
            ));
        }

        let result = cx.update_global::<gpui_query::client::QueryClient, _>(|client, cx| {
            client.fetch_query::<RawResponse, QueryError>(
                key,
                CachePolicy::NoCache,
                RequestPolicy::LatestWins,
                now_ms,
                cx,
            )
        });

        match result {
            Some((_entity, request_id)) => {
                let v_started = Self::verdict("request started", true, &format!("request_id={}", request_id.label()));
                let verdict_line = "Client fetch PASSED";
                self.client_query_message = format!("{v_started}\n{verdict_line}");
            }
            None => {
                let v_started = Self::verdict("request started", false, "returned None (cache hit or ignored)");
                let verdict_line = "Client fetch FAILED";
                self.client_query_message = format!("{v_started}\n{verdict_line}");
            }
        }
        cx.notify();
    }

    fn exercise_client_force_fetch_query(&mut self, cx: &mut Context<Self>) {
        let key = gpui_query::QueryKey::from_single("http_lab_testing/client_force_fetch");
        let now_ms = query_now_ms();

        if !cx.has_global::<gpui_query::client::QueryClient>() {
            cx.set_global(gpui_query::client::QueryClient::new(
                gpui_query::CachePolicy::default(),
                gpui_query::RequestPolicy::default(),
            ));
        }

        let result = cx.update_global::<gpui_query::client::QueryClient, _>(|client, cx| {
            client.force_fetch_query::<RawResponse, QueryError>(
                key,
                CachePolicy::NoCache,
                RequestPolicy::LatestWins,
                now_ms,
                cx,
            )
        });

        match result {
            Some((_entity, request_id)) => {
                let v_started = Self::verdict("forced request started", true, &format!("request_id={}", request_id.label()));
                let verdict_line = "Client force fetch PASSED";
                self.client_query_message = format!("{v_started}\n{verdict_line}");
            }
            None => {
                let v_started = Self::verdict("forced request started", false, "returned None (ignored)");
                let verdict_line = "Client force fetch FAILED";
                self.client_query_message = format!("{v_started}\n{verdict_line}");
            }
        }
        cx.notify();
    }

    fn toggle_query_details(&mut self, cx: &mut Context<Self>) {
        self.show_query_details = !self.show_query_details;
        cx.notify();
    }

    fn toggle_signal_details(&mut self, cx: &mut Context<Self>) {
        self.show_signal_details = !self.show_signal_details;
        cx.notify();
    }

    fn toggle_retention_details(&mut self, cx: &mut Context<Self>) {
        self.show_retention_details = !self.show_retention_details;
        cx.notify();
    }

    fn toggle_optimistic_details(&mut self, cx: &mut Context<Self>) {
        self.show_optimistic_details = !self.show_optimistic_details;
        cx.notify();
    }

    fn toggle_client_details(&mut self, cx: &mut Context<Self>) {
        self.show_client_details = !self.show_client_details;
        cx.notify();
    }

    fn toggle_local_history(&mut self, cx: &mut Context<Self>) {
        self.show_local_history = !self.show_local_history;
        cx.notify();
    }

    fn toggle_response_preview(&mut self, cx: &mut Context<Self>) {
        self.show_response_preview = !self.show_response_preview;
        cx.notify();
    }

    fn toggle_response_details(&mut self, cx: &mut Context<Self>) {
        self.show_response_details = !self.show_response_details;
        cx.notify();
    }
}

impl Render for HttpLabTestingPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = Instant::now();
        let is_sending = matches!(self.status, RawStatus::Sending);
        let active_operation_id = self.active_operation_id;
        let query_status = self.query_resource.status();
        let local_selected = self.local_lab_selected;
        let local_history_len = self.local_lab_history.len();

        // -- Section 1: Query Lifecycle --
        let query_lifecycle_section = section_card(
            "Query Lifecycle",
            "Test request policies (LatestWins, IgnoreWhileLoading) and cache TTL behavior",
            cx,
        )
        .child(
            div().flex().flex_wrap().gap_2().px_4().py_3()
            .child(
                Button::new("http-lab-testing-query-send")
                    .outline()
                    .label(if is_sending {
                        "Sending query GET"
                    } else {
                        "Send query GET"
                    })
                    .disabled(is_sending)
                    .tooltip("Real HTTP GET through QueryResource (NoCache, LatestWins). Result appears in the resource state below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send_query_get(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-query-ttl")
                    .outline()
                    .label("Query TTL")
                    .disabled(is_sending)
                    .tooltip("Sync probe: start\u{2192}complete\u{2192}start again. Second should hit cache. Look for cache_hit=true below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_ttl_cache(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-query-ignore")
                    .outline()
                    .label("Query ignore")
                    .disabled(is_sending)
                    .tooltip("Sync probe: start request, try duplicate. Duplicate ignored. Look for duplicate_ignored=true below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_ignore_while_loading(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-query-latest")
                    .outline()
                    .label("Query latest")
                    .disabled(is_sending)
                    .tooltip("Sync probe: two requests, second replaces first. Stale completion rejected. Look for stale_accepted=false below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_latest_wins(cx);
                    })),
            ),
        )
        .child(
            div().px_4().pb_3().child(
                toggle_button(
                    "http-lab-testing-toggle-query-details",
                    "Query details",
                    self.show_query_details,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_query_details(cx))),
            ),
        )
        .when(self.show_query_details, |section| {
            section.child(
                div().px_4().py_3().child(
                    v_flex().gap_2()
                        .child(query_resource_row("Main", &self.query_resource, cx))
                        .child(query_resource_row("TTL", &self.query_ttl_resource, cx))
                        .child(query_resource_row("Ignore", &self.query_ignore_resource, cx))
                        .child(query_resource_row("Latest", &self.query_latest_resource, cx))
                        .child(row("Query message", &self.query_message, cx)),
                ),
            )
        });

        // -- Section 2: Cancel Signal --
        let signal_resource = &self.query_signal_resource;
        let signal_status = match signal_resource.signal() {
            Some(signal) => {
                if signal.is_cancelled() {
                    "cancelled"
                } else {
                    "active"
                }
            }
            None => "none",
        };
        let cancel_signal_section = section_card(
            "Cancel Signal",
            "Test cooperative cancellation signal that propagates to cloned signal references",
            cx,
        )
        .child(
            div().flex().flex_wrap().gap_2().px_4().py_3()
            .child(
                Button::new("http-lab-testing-query-signal")
                    .outline()
                    .label("Query signal")
                    .disabled(is_sending)
                    .tooltip("Sync probe: begin request, clone signal, cancel resource. Cloned signal should read is_cancelled=true. Look for before=false after=true below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_signal(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-signal-cancel")
                    .danger()
                    .outline()
                    .label("Cancel active")
                    .disabled(!is_sending)
                    .tooltip("Cancels any in-flight request via CancellationToken.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel(cx);
                    })),
            ),
        )
        .child(
            div().px_4().pb_3().child(
                toggle_button(
                    "http-lab-testing-toggle-signal-details",
                    "Signal details",
                    self.show_signal_details,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_signal_details(cx))),
            ),
        )
        .when(self.show_signal_details, |section| {
            section.child(
                div().px_4().py_3().child(
                    v_flex().gap_2()
                        .child(query_resource_row("Signal resource", signal_resource, cx))
                        .child(row("Signal", signal_status, cx))
                        .child(row("Signal message", &self.query_signal_message, cx)),
                ),
            )
        });

        // -- Section 3: Cache & Data Retention --
        let placeholder_resource = &self.query_placeholder_resource;
        let ph_data = compact_resource_preview(placeholder_resource.data());
        let ph_placeholder = compact_resource_preview(placeholder_resource.placeholder_data());
        let ph_display = compact_resource_preview(placeholder_resource.display_data());
        let ph_previous = compact_resource_preview(placeholder_resource.previous_data());

        let data_retention_section = section_card(
            "Cache & Data Retention",
            "Test placeholder data fallback, automatic previous_data tracking on success, and rollback",
            cx,
        )
        .child(
            div().flex().flex_wrap().gap_2().px_4().py_3()
            .child(
                Button::new("http-lab-testing-placeholder")
                    .outline()
                    .label("Placeholder data")
                    .disabled(is_sending)
                    .tooltip("Sync probe: seed data, set placeholder, reset, begin loading. display_data returns placeholder during loading, real data after completion. Check results below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_placeholder_data(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-previous-data")
                    .outline()
                    .label("Previous data")
                    .disabled(is_sending)
                    .tooltip("Sync probe: seed 'first', then 'second'. previous_data holds 'first'. Check results below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_previous_data(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-rollback")
                    .outline()
                    .label("Rollback")
                    .disabled(is_sending)
                    .tooltip("Sync probe: seed data, overwrite, rollback_to_previous. Data restored. Check results below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_rollback(cx);
                    })),
            ),
        )
        .child(
            div().px_4().pb_3().child(
                toggle_button(
                    "http-lab-testing-toggle-retention-details",
                    "Retention details",
                    self.show_retention_details,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_retention_details(cx))),
            ),
        )
        .when(self.show_retention_details, |section| {
            section.child(
                div().px_4().py_3().child(
                    v_flex().gap_2()
                        .child(query_resource_row("Placeholder resource", placeholder_resource, cx))
                        .child(row("Data", &ph_data, cx))
                        .child(row("Placeholder", &ph_placeholder, cx))
                        .child(row("Display data", &ph_display, cx))
                        .child(row("Previous data", &ph_previous, cx))
                        .child(row("Placeholder message", &self.query_placeholder_message, cx)),
                ),
            )
        });

        // -- Section 4: Optimistic Updates --
        let optimistic_resource = &self.query_optimistic_resource;
        let opt_data = compact_resource_preview(optimistic_resource.data());
        let opt_previous = compact_resource_preview(optimistic_resource.previous_data());
        let opt_display = compact_resource_preview(optimistic_resource.display_data());
        let opt_status = optimistic_resource.status().label().to_string();

        let optimistic_section = section_card(
            "Optimistic Updates",
            "Test optimistic writes that store previous data for rollback on mutation failure",
            cx,
        )
        .child(
            div().flex().flex_wrap().gap_2().px_4().py_3()
            .child(
                Button::new("http-lab-testing-optimistic-set")
                    .outline()
                    .label("Optimistic set")
                    .disabled(is_sending)
                    .tooltip("Sync probe: seed 'original', set_data('optimistic'). data='optimistic' previous='original'. Status unchanged.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_optimistic_set(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-optimistic-rollback")
                    .outline()
                    .label("Optimistic rollback")
                    .disabled(is_sending)
                    .tooltip("Sync probe: seed, set_data, rollback. Data restored to original.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_optimistic_rollback(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-optimistic-flow")
                    .outline()
                    .label("Full mutation")
                    .disabled(is_sending)
                    .tooltip("Sync probe: seed 'original' \u{2192} set_data('optimistic') \u{2192} complete('server confirmed'). data='server confirmed' previous='optimistic'.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_query_optimistic_flow(cx);
                    })),
            ),
        )
        .child(
            div().px_4().pb_3().child(
                toggle_button(
                    "http-lab-testing-toggle-optimistic-details",
                    "Optimistic details",
                    self.show_optimistic_details,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_optimistic_details(cx))),
            ),
        )
        .when(self.show_optimistic_details, |section| {
            section.child(
                div().px_4().py_3().child(
                    v_flex().gap_2()
                        .child(query_resource_row("Optimistic resource", optimistic_resource, cx))
                        .child(row("Data", &opt_data, cx))
                        .child(row("Previous data", &opt_previous, cx))
                        .child(row("Display data", &opt_display, cx))
                        .child(row("Status", &opt_status, cx))
                        .child(row("Optimistic message", &self.query_optimistic_message, cx)),
                ),
            )
        });

        // -- Section 5: Standalone Client Fetch --
        let client_fetch_section = section_card(
            "Standalone Client Fetch",
            "Test QueryClient.fetch_query() and force_fetch_query() \u{2014} no component subscription needed",
            cx,
        )
        .child(
            div().flex().flex_wrap().gap_2().px_4().py_3()
            .child(
                Button::new("http-lab-testing-client-fetch")
                    .outline()
                    .label("Client fetch")
                    .disabled(is_sending)
                    .tooltip("Imperative fetch via QueryClient. Creates resource and starts request without a component. Check message below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_client_fetch_query(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-client-force")
                    .outline()
                    .label("Client force fetch")
                    .disabled(is_sending)
                    .tooltip("Same but bypasses cache freshness checks. Check message below.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.exercise_client_force_fetch_query(cx);
                    })),
            ),
        )
        .child(
            div().px_4().pb_3().child(
                toggle_button(
                    "http-lab-testing-toggle-client-details",
                    "Client details",
                    self.show_client_details,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_client_details(cx))),
            ),
        )
        .when(self.show_client_details, |section| {
            section.child(
                div().px_4().py_3().child(
                    v_flex()
                        .gap_2()
                        .child(row("Client message", &self.client_query_message, cx)),
                ),
            )
        });

        // -- Section 6: Local Full Lab --
        let local_lab_section = section_card(
            "Local Full Lab",
            "Full integration: each action uses its own QueryResource with real cache/request policies, real HTTP calls",
            cx,
        )
        .child(
            div().flex().flex_wrap().gap_2().px_4().py_3()
            .children(HttpLabAction::all().iter().copied().map(|action| {
                let tip = match action {
                    HttpLabAction::GetText => "Sends GET to httpbingo.org/encoding/utf8 (TTL 60s, LatestWins). Populates the local lab resource panel.",
                    HttpLabAction::GetXml => "Sends GET to httpbingo.org/xml (TTL 60s, LatestWins). Populates the local lab resource panel.",
                    HttpLabAction::GetJson => "Sends GET to httpbingo.org/json (StaleWhileRevalidate 30s, LatestWins). Populates the local lab resource panel.",
                    HttpLabAction::PostJson => "Sends POST to httpbingo.org/post (NoCache, LatestWins). Populates the local lab resource panel.",
                    HttpLabAction::PostForm => "Sends POST to httpbingo.org/post (NoCache, LatestWins). Populates the local lab resource panel.",
                    HttpLabAction::PostMultipart => "Sends POST to httpbingo.org/post (NoCache, IgnoreWhileLoading). Duplicates are ignored while loading.",
                    HttpLabAction::Cookies => "Sends GET to httpbingo.org/cookies (NoCache, LatestWins). Populates the local lab resource panel.",
                    HttpLabAction::Failure => "Sends GET to httpbingo.org/status/418 (NoCache, LatestWins). Expect a 418 error response.",
                    HttpLabAction::FullFlow => "Runs 4 sequential requests (GetJson, PostJson, Cookies, Failure) and populates all individual resources plus the FullFlow resource.",
                };
                Button::new(format!("http-lab-testing-local-{}", action.id()))
                    .outline()
                    .label(format!("Local {}", action.label()))
                    .disabled(is_sending)
                    .tooltip(tip)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.send_local_lab_action(action, cx);
                    }))
            }))
            .child(
                Button::new("http-lab-testing-local-reset")
                    .outline()
                    .label("Local reset")
                    .disabled(is_sending)
                    .tooltip("Resets all local lab resources to Idle, advances the request sequencer scope, and clears history.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reset_local_lab(cx);
                    })),
            )
            .child(
                Button::new("http-lab-testing-local-cancel")
                    .danger()
                    .outline()
                    .label("Cancel active")
                    .disabled(!is_sending)
                    .tooltip("Cancels any in-flight request via CancellationToken.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel(cx);
                    })),
            ),
        )
        .child(
            div().px_4().py_3().child(
                v_flex()
                    .gap_2()
                    .child(row("Selected", self.local_lab_selected.label(), cx))
                    .child(row("Message", &self.local_lab_message, cx))
                    .child(row(
                        "History",
                        &self.local_lab_history.len().to_string(),
                        cx,
                    ))
                    .child(toggle_button(
                        "http-lab-testing-toggle-local-history",
                        "History details",
                        self.show_local_history,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_local_history(cx))))
                    .children(HttpLabAction::all().iter().copied().map(|action| {
                        let resource = self
                            .local_lab_resources
                            .get(&action)
                            .expect("local lab resource must exist");
                        query_resource_row(action.label(), resource, cx)
                    }))
                    .when(self.show_local_history, |this| {
                        this.child(local_lab_history_panel(self, cx))
                    }),
            ),
        );

        // -- Section 7: Raw Baseline --
        let raw_baseline_section = section_card(
            "Raw Baseline",
            "Plain reqwest GET with manual operation tracking \u{2014} no gpui-query involved",
            cx,
        )
        .child(
            div().flex().flex_wrap().gap_2().px_4().py_3()
            .child(
                Button::new("http-lab-testing-send")
                    .outline()
                    .label(if is_sending {
                        "Sending raw GET"
                    } else {
                        "Send raw GET"
                    })
                    .disabled(is_sending)
                    .tooltip("Plain reqwest GET to httpbingo.org. No QueryResource. Baseline for comparison.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send_raw_get(cx);
                    })),
            ),
        )
        .child(
            div().px_4().py_3().child(
                v_flex()
                    .gap_2()
                    .child(row("Status", self.status.label(), cx))
                    .child(row(
                        "Active operation",
                        &self
                            .active_operation_id
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "none".to_string()),
                        cx,
                    ))
                    .child(row("Message", &self.last_message, cx))
                    .child(toggle_button(
                        "http-lab-testing-toggle-response-details",
                        "Response details",
                        self.show_response_details,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_response_details(cx))))
                    .when(self.show_response_details, |this| {
                        this.when_some(self.last_response.as_ref(), |this, response| {
                            this.child(row("Response status", &response.status.to_string(), cx))
                                .child(row("Response URL", &response.final_url, cx))
                                .child(row(
                                    "Response headers",
                                    &response.header_count.to_string(),
                                    cx,
                                ))
                                .child(row("Preview bytes", &response.bytes.to_string(), cx))
                                .child(toggle_button(
                                    "http-lab-testing-toggle-response-preview",
                                    "Response preview",
                                    self.show_response_preview,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_response_preview(cx))))
                                .when(self.show_response_preview, |this| {
                                    this.child(
                                        div()
                                            .p_3()
                                            .rounded(cx.theme().radius)
                                            .bg(cx.theme().muted)
                                            .text_xs()
                                            .font_family("monospace")
                                            .child(preview_excerpt(&response.preview, 1024)),
                                    )
                                })
                        })
                    })
                    .when(self.last_response.is_none(), |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("No response captured."),
                        )
                    }),
            ),
        );

        let view = v_flex()
            .min_h_full()
            .p_6()
            .gap_5()
            .child(
                div()
                    .p_5()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().muted)
                    .child(
                        v_flex()
                            .gap_3()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight::BOLD)
                                    .child("HTTP Lab Testing"),
                            )
                            .child(
                                div()
                                    .max_w(px(760.))
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Raw reqwest-only screen for isolating GPUI task scheduling from the existing HTTP Lab store and gpui-query path."),
                            ),
                    ),
            )
            .child(query_lifecycle_section)
            .child(cancel_signal_section)
            .child(data_retention_section)
            .child(optimistic_section)
            .child(client_fetch_section)
            .child(local_lab_section)
            .child(raw_baseline_section);

        tracing::debug!(
            target: RENDER_LOG,
            elapsed_us = render_started.elapsed().as_micros() as u64,
            status = self.status.label(),
            is_sending,
            active_operation_id,
            query_status = query_status.label(),
            local_selected = local_selected.id(),
            local_history_len,
            "HTTP Lab Testing render completed"
        );

        view
    }
}

async fn raw_reqwest_get(
    client: reqwest::Client,
    url: String,
    cancellation: CancellationToken,
    operation_id: u64,
) -> Result<RawResponse, String> {
    tracing::info!(
        target: LOG,
        operation_id,
        url,
        "HTTP Lab Testing raw request build started"
    );

    let request = client
        .get(&url)
        .header("accept", "application/json")
        .header("x-gpui-http-lab-testing", operation_id.to_string());

    tracing::info!(
        target: LOG,
        operation_id,
        "HTTP Lab Testing raw request send started"
    );

    let send_started = Instant::now();
    let mut response = tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
            tracing::info!(
                target: LOG,
                operation_id,
                "HTTP Lab Testing raw request cancelled before response"
            );
            return Err("cancelled".to_string());
        }
        result = request.send() => result.map_err(|err| err.to_string())?,
    };

    let send_elapsed_ms = send_started.elapsed().as_millis();
    let status = response.status().as_u16();
    let final_url = response.url().to_string();
    let header_count = response.headers().len();

    tracing::info!(
        target: LOG,
        operation_id,
        status,
        final_url,
        header_count,
        send_elapsed_ms,
        "HTTP Lab Testing raw request send completed"
    );

    let mut bytes = Vec::new();
    let body_started = Instant::now();
    loop {
        if bytes.len() >= PREVIEW_LIMIT {
            break;
        }

        let chunk = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                tracing::info!(
                    target: LOG,
                    operation_id,
                    bytes = bytes.len(),
                    "HTTP Lab Testing raw body cancelled"
                );
                return Err("cancelled".to_string());
            }
            result = response.chunk() => result.map_err(|err| err.to_string())?,
        };

        let Some(chunk) = chunk else {
            break;
        };

        let remaining = PREVIEW_LIMIT - bytes.len();
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }

    let body_elapsed_ms = body_started.elapsed().as_millis();
    let preview = String::from_utf8_lossy(&bytes).to_string();

    tracing::info!(
        target: LOG,
        operation_id,
        bytes = bytes.len(),
        body_elapsed_ms,
        "HTTP Lab Testing raw body preview completed"
    );

    Ok(RawResponse {
        status,
        final_url,
        header_count,
        bytes: bytes.len(),
        preview,
    })
}

async fn run_local_lab_action(
    client: reqwest::Client,
    action: HttpLabAction,
    cancellation: CancellationToken,
    operation_id: u64,
) -> Result<Vec<(HttpLabAction, RawResponse)>, String> {
    if action == HttpLabAction::FullFlow {
        let mut exchanges = Vec::new();
        for target_action in [
            HttpLabAction::GetJson,
            HttpLabAction::PostJson,
            HttpLabAction::Cookies,
            HttpLabAction::Failure,
        ] {
            let response = raw_reqwest_get(
                client.clone(),
                local_lab_url(target_action),
                cancellation.clone(),
                operation_id,
            )
            .await?;
            exchanges.push((target_action, response));
        }
        return Ok(exchanges);
    }

    let response = raw_reqwest_get(
        client,
        local_lab_url(action),
        cancellation.clone(),
        operation_id,
    )
    .await?;
    Ok(vec![(action, response)])
}

fn local_lab_url(action: HttpLabAction) -> String {
    match action {
        HttpLabAction::GetText => "https://httpbin.org/encoding/utf8".to_string(),
        HttpLabAction::GetJson => "https://httpbin.org/json".to_string(),
        HttpLabAction::GetXml => "https://httpbin.org/xml".to_string(),
        HttpLabAction::PostJson => "https://httpbin.org/post?local=post_json".to_string(),
        HttpLabAction::PostForm => "https://httpbin.org/post?local=post_form".to_string(),
        HttpLabAction::PostMultipart => "https://httpbin.org/post?local=multipart".to_string(),
        HttpLabAction::Cookies => "https://httpbin.org/cookies".to_string(),
        HttpLabAction::Failure => "https://httpbin.org/status/418".to_string(),
        HttpLabAction::FullFlow => TEST_URL.to_string(),
    }
}

fn section_card(title: &str, description: &str, cx: &App) -> Div {
    div()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .overflow_hidden()
        .child(
            div()
                .px_4()
                .py_3()
                .bg(cx.theme().muted)
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title.to_string()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(description.to_string()),
                        ),
                ),
        )
}

fn toggle_button(id: &str, label: &str, expanded: bool) -> Button {
    Button::new(id.to_string())
        .ghost()
        .label(if expanded {
            format!("Hide {label}")
        } else {
            format!("Show {label}")
        })
        .tooltip("Toggles heavy debug content for this section.")
}

fn preview_excerpt(value: &str, limit: usize) -> String {
    let truncated: String = value.chars().take(limit).collect();
    if value.chars().count() > limit {
        format!("{truncated}\n... truncated")
    } else {
        truncated
    }
}

fn compact_resource_preview(resource: Option<&RawResponse>) -> String {
    match resource {
        Some(response) => format!(
            "status={} bytes={} preview=\"{}\"",
            response.status,
            response.bytes,
            preview_excerpt(&response.preview, 96).replace('\n', " ")
        ),
        None => "none".to_string(),
    }
}

fn local_lab_history_panel(page: &HttpLabTestingPage, cx: &App) -> Div {
    let render_started = Instant::now();
    let mut body = v_flex().gap_1();
    for (action, response) in page.local_lab_history.iter().take(6) {
        body = body.child(div().text_xs().font_family("monospace").child(format!(
            "{} status={} bytes={} url={}",
            action.label(),
            response.status,
            response.bytes,
            response.final_url
        )));
    }

    let view = div()
        .p_3()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .child(if page.local_lab_history.is_empty() {
            v_flex().child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("No local lab history."),
            )
        } else {
            body
        });

    tracing::debug!(
        target: RENDER_LOG,
        elapsed_us = render_started.elapsed().as_micros() as u64,
        history_len = page.local_lab_history.len(),
        rendered_rows = page.local_lab_history.len().min(6),
        "HTTP Lab Testing local lab history panel rendered"
    );

    view
}

fn query_resource_row(label: &str, resource: &QueryResource<RawResponse>, cx: &App) -> Div {
    let active = resource
        .active_request_id()
        .map(|id| id.label())
        .unwrap_or_else(|| "none".to_string());
    let data = if resource.data().is_some() {
        "data"
    } else {
        "no data"
    };
    row(
        label,
        &format!("{} active={} {}", resource.status().label(), active, data),
        cx,
    )
}

fn row(label: &str, value: &str, cx: &App) -> Div {
    div()
        .flex()
        .gap_3()
        .items_start()
        .child(
            div()
                .w(px(140.))
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(div().flex_1().text_sm().child(value.to_string()))
}

fn query_now_ms() -> u128 {
    static STARTED_AT: OnceLock<Instant> = OnceLock::new();
    STARTED_AT.get_or_init(Instant::now).elapsed().as_millis()
}

fn fake_response(label: &str) -> RawResponse {
    RawResponse {
        status: 200,
        final_url: format!("memory://{label}"),
        header_count: 0,
        bytes: label.len(),
        preview: label.to_string(),
    }
}

fn local_lab_resources() -> BTreeMap<HttpLabAction, QueryResource<RawResponse>> {
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
