use std::time::Instant;

use gpui::*;
use tokio_util::sync::CancellationToken;

use gpui_query::{QueryBeginResult, QueryError, QueryFetchMode};

use crate::services::{http_lab::HttpLabAction, tokio_runtime::TokioRuntimeGlobal};

use super::super::{local_lab_resources, query_now_ms, RawResponse, RawStatus, LOG};
use super::super::network::run_local_lab_action;

impl super::super::HttpLabTestingPage {
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
}
