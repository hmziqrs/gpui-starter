use std::time::Instant;

use gpui::*;
use tokio_util::sync::CancellationToken;

use gpui_query::{QueryBeginResult, QueryError, QueryFetchMode};

use crate::services::tokio_runtime::TokioRuntimeGlobal;

use super::super::network::raw_reqwest_get;
use super::super::{LOG, RawStatus, TEST_URL, fake_response, query_now_ms};

impl super::super::HttpLabTestingPage {
    pub(crate) fn send_query_get(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn exercise_query_ttl_cache(&mut self, cx: &mut Context<Self>) {
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
        let v_accepted = Self::verdict(
            "first request started",
            accepted,
            &format!("accepted={accepted}"),
        );
        let v_cache_hit = Self::verdict(
            "cache hit on retry",
            cache_hit,
            &format!("cache_hit={cache_hit}"),
        );
        let all_passed = accepted && cache_hit;
        let verdict_line = if all_passed {
            "TTL cache probe PASSED"
        } else {
            "TTL cache probe FAILED"
        };
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

    pub(crate) fn exercise_query_ignore_while_loading(&mut self, cx: &mut Context<Self>) {
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
        let v_cancelled = Self::verdict(
            "cleanup cancelled",
            cancelled,
            &format!("cancelled={cancelled}"),
        );
        let all_passed = ignored && cancelled;
        let verdict_line = if all_passed {
            "Ignore-while-loading probe PASSED"
        } else {
            "Ignore-while-loading probe FAILED"
        };
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

    pub(crate) fn exercise_query_latest_wins(&mut self, cx: &mut Context<Self>) {
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
        let v_replaced = Self::verdict(
            "second replaced first",
            replaced,
            &format!("replaced={:?}", replaced_request_id.map(|id| id.label())),
        );
        let v_stale = Self::verdict(
            "stale rejected",
            !stale_accepted,
            &format!("stale_accepted={stale_accepted}"),
        );
        let v_latest = Self::verdict(
            "latest accepted",
            latest_accepted,
            &format!("latest_accepted={latest_accepted}"),
        );
        let all_passed = replaced && !stale_accepted && latest_accepted;
        let verdict_line = if all_passed {
            "Latest-wins probe PASSED"
        } else {
            "Latest-wins probe FAILED"
        };
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
}
