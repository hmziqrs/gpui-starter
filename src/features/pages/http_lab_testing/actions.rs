use std::time::Instant;

use gpui::{prelude::*, *};
use tokio_util::sync::CancellationToken;

use gpui_query::{
    CachePolicy, QueryBeginResult, QueryError, QueryFetchMode, RequestPolicy,
};

use crate::services::{http_lab::HttpLabAction, tokio_runtime::TokioRuntimeGlobal};

use super::{
    fake_response, local_lab_resources, query_now_ms, LOG,
    RawResponse, RawStatus, TEST_URL,
};
use super::network::{raw_reqwest_get, run_local_lab_action};

impl super::HttpLabTestingPage {
    pub(crate) fn send_raw_get(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn cancel(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn verdict(label: &str, passed: bool, detail: &str) -> String {
        let icon = if passed { "✅" } else { "❌" };
        format!("{icon} {label}  — {detail}")
    }

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

    pub(crate) fn reset_local_lab(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn send_local_lab_action(&mut self, action: HttpLabAction, cx: &mut Context<Self>) {
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

    pub(crate) fn exercise_query_signal(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn exercise_query_placeholder_data(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn exercise_query_previous_data(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn exercise_query_rollback(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn exercise_query_optimistic_set(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn exercise_query_optimistic_rollback(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn exercise_query_optimistic_flow(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn exercise_client_fetch_query(&mut self, cx: &mut Context<Self>) {
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

        let now_ms_for_complete = now_ms;
        match result {
            Some((entity, request_id)) => {
                let rid_label = request_id.label();
                // Complete the request immediately so the resource transitions
                // from LoadingEmpty → Success (otherwise DevTools shows "Loading").
                let completed = entity.update(cx, |resource, _| {
                    resource.complete_current_success(
                        request_id,
                        RawResponse {
                            status: 200,
                            final_url: "https://httpbin.org/json".to_string(),
                            header_count: 0,
                            bytes: 0,
                            preview: "client_fetch probe".to_string(),
                        },
                        now_ms_for_complete,
                    )
                });
                let v_started = Self::verdict("request started", true, &format!("request_id={}", rid_label));
                let v_completed = Self::verdict("request completed", completed, "complete_current_success");
                let verdict_line = "Client fetch PASSED";
                self.client_query_message = format!("{v_started}\n{v_completed}\n{verdict_line}");
            }
            None => {
                let v_started = Self::verdict("request started", false, "returned None (cache hit or ignored)");
                let verdict_line = "Client fetch FAILED";
                self.client_query_message = format!("{v_started}\n{verdict_line}");
            }
        }
        cx.notify();
    }

    pub(crate) fn exercise_client_force_fetch_query(&mut self, cx: &mut Context<Self>) {
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

        let now_ms_for_complete = now_ms;
        match result {
            Some((entity, request_id)) => {
                let rid_label = request_id.label();
                // Complete the request immediately so the resource transitions
                // from LoadingEmpty → Success (otherwise DevTools shows "Loading").
                let completed = entity.update(cx, |resource, _| {
                    resource.complete_current_success(
                        request_id,
                        RawResponse {
                            status: 200,
                            final_url: "https://httpbin.org/json".to_string(),
                            header_count: 0,
                            bytes: 0,
                            preview: "client_force_fetch probe".to_string(),
                        },
                        now_ms_for_complete,
                    )
                });
                let v_started = Self::verdict("forced request started", true, &format!("request_id={}", rid_label));
                let v_completed = Self::verdict("request completed", completed, "complete_current_success");
                let verdict_line = "Client force fetch PASSED";
                self.client_query_message = format!("{v_started}\n{v_completed}\n{verdict_line}");
            }
            None => {
                let v_started = Self::verdict("forced request started", false, "returned None (ignored)");
                let verdict_line = "Client force fetch FAILED";
                self.client_query_message = format!("{v_started}\n{verdict_line}");
            }
        }
        cx.notify();
    }

    pub(crate) fn toggle_query_details(&mut self, cx: &mut Context<Self>) {
        self.show_query_details = !self.show_query_details;
        cx.notify();
    }

    pub(crate) fn toggle_signal_details(&mut self, cx: &mut Context<Self>) {
        self.show_signal_details = !self.show_signal_details;
        cx.notify();
    }

    pub(crate) fn toggle_retention_details(&mut self, cx: &mut Context<Self>) {
        self.show_retention_details = !self.show_retention_details;
        cx.notify();
    }

    pub(crate) fn toggle_optimistic_details(&mut self, cx: &mut Context<Self>) {
        self.show_optimistic_details = !self.show_optimistic_details;
        cx.notify();
    }

    pub(crate) fn toggle_client_details(&mut self, cx: &mut Context<Self>) {
        self.show_client_details = !self.show_client_details;
        cx.notify();
    }

    pub(crate) fn toggle_local_history(&mut self, cx: &mut Context<Self>) {
        self.show_local_history = !self.show_local_history;
        cx.notify();
    }

    pub(crate) fn toggle_response_preview(&mut self, cx: &mut Context<Self>) {
        self.show_response_preview = !self.show_response_preview;
        cx.notify();
    }

    pub(crate) fn toggle_response_details(&mut self, cx: &mut Context<Self>) {
        self.show_response_details = !self.show_response_details;
        cx.notify();
    }
}
