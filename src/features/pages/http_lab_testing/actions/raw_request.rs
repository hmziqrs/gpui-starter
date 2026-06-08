use gpui::{prelude::*, *};
use tokio_util::sync::CancellationToken;

use crate::services::tokio_runtime::TokioRuntimeGlobal;

use super::super::{RawStatus, LOG, TEST_URL};
use super::super::network::raw_reqwest_get;

impl super::super::HttpLabTestingPage {
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

            let started = std::time::Instant::now();
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
}
