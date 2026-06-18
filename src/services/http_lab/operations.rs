use std::{sync::OnceLock, time::Instant};

use gpui::{App, BorrowAppContext as _};

use crate::services::{
    http_lab::{
        client::run_http_action,
        state::{HttpLabState, ResetRequests},
        task_tracking::{cancel_request_flag, register_request_flag},
        transitions::{
            apply_result_to_state, begin_action, cancel_action_in_state, cancel_all_in_state,
            prepare_retry, should_retry_action,
        },
        types::{ActionExchange, HttpLabAction},
    },
    tokio_runtime::TokioRuntimeGlobal,
};
use gpui_query_legacy::{QueryBeginResult, QueryFetchMode, RequestId};

const LOG: &str = "gpui_starter::http_lab";

pub fn initialize(cx: &mut App) {
    cx.set_global(HttpLabState::default());
    crate::capabilities::set(
        "http_lab",
        crate::capabilities::CapabilityStatus::supported_enabled(),
        cx,
    );
}

pub fn snapshot(cx: &App) -> HttpLabState {
    cx.try_global::<HttpLabState>().cloned().unwrap_or_default()
}

pub fn read_state<R>(cx: &App, read: impl FnOnce(&HttpLabState) -> R) -> R {
    if let Some(state) = cx.try_global::<HttpLabState>() {
        read(state)
    } else {
        let fallback = HttpLabState::default();
        read(&fallback)
    }
}

pub fn reset(cx: &mut App) {
    tracing::info!(target: LOG, "HTTP Lab reset requested");
    let reset_requests = if cx.try_global::<HttpLabState>().is_some() {
        cx.update_global::<HttpLabState, _>(|state, _cx| state.reset_for_user())
    } else {
        cx.set_global(HttpLabState::default());
        ResetRequests::default()
    };

    for request_id in reset_requests.request_ids {
        tracing::debug!(
            target: LOG,
            request_id = %request_id.label(),
            "HTTP Lab reset cancelling request token"
        );
        cancel_request_flag(request_id);
    }
}

pub fn select_action(action: HttpLabAction, cx: &mut App) {
    cx.update_global::<HttpLabState, _>(|state, _cx| {
        state.selected_action = action;
    });
}

/// Prepare an HTTP action: update state, register task, return handles for spawning.
/// Returns `None` if the action was deduplicated (cache hit or already loading).
pub fn prepare_action(action: HttpLabAction, cx: &mut App) -> Option<ActionHandle> {
    let now_ms = now_ms();
    tracing::info!(
        target: LOG,
        action = action.id(),
        now_ms,
        "HTTP Lab preparing action"
    );
    let request_id =
        cx.update_global::<HttpLabState, _>(|state, _cx| begin_action(state, action, now_ms))?;

    tracing::info!(
        target: LOG,
        action = action.id(),
        request_id = %request_id.label(),
        "HTTP Lab action accepted"
    );
    let cancellation = register_request_flag(request_id);
    tracing::debug!(
        target: LOG,
        action = action.id(),
        request_id = %request_id.label(),
        "HTTP Lab fetching Tokio runtime global"
    );
    let rt = cx.global::<TokioRuntimeGlobal>().0.runtime.clone();
    tracing::debug!(
        target: LOG,
        action = action.id(),
        request_id = %request_id.label(),
        "HTTP Lab fetching reqwest client global"
    );
    let client = cx.global::<TokioRuntimeGlobal>().0.http_client.clone();
    let request_cancellation = cancellation.clone();
    tracing::info!(
        target: LOG,
        action = action.id(),
        request_id = %request_id.label(),
        "HTTP Lab spawning Tokio request task immediately"
    );
    let http_handle = rt.spawn(async move {
        tracing::info!(
            target: LOG,
            action = action.id(),
            request_id = %request_id.label(),
            "HTTP Lab Tokio request task started"
        );
        let result = run_http_action(&client, action, request_cancellation).await;
        match &result {
            Ok(exchanges) => tracing::info!(
                target: LOG,
                action = action.id(),
                request_id = %request_id.label(),
                exchange_count = exchanges.len(),
                "HTTP Lab Tokio request task completed"
            ),
            Err(error) => tracing::warn!(
                target: LOG,
                action = action.id(),
                request_id = %request_id.label(),
                error = %error,
                "HTTP Lab Tokio request task failed"
            ),
        }
        result
    });
    tracing::info!(
        target: LOG,
        action = action.id(),
        request_id = %request_id.label(),
        "HTTP Lab action handle prepared"
    );

    Some(ActionHandle {
        action,
        request_id,
        cancellation,
        http_handle,
    })
}

/// Run a prepared action. The caller must spawn this on a GPUI entity context
/// so that `cx.update` can push results back into the view.
///
/// Loops automatically on retryable failures: `apply_result` returns a retry
/// handle when the retry policy permits another attempt, and we await that
/// handle on the same entity task, feeding the result back through
/// `apply_result` until no further retries are needed.
pub async fn execute_action(handle: ActionHandle, cx: &mut gpui::AsyncApp) {
    let mut current_handle = handle;

    loop {
        let started = Instant::now();
        let ActionHandle {
            action,
            request_id,
            cancellation,
            http_handle,
        } = current_handle;
        tracing::info!(
            target: LOG,
            action = action.id(),
            request_id = %request_id.label(),
            cancelled = cancellation.is_cancelled(),
            "HTTP Lab awaiting pre-spawned Tokio request task"
        );

        let result = http_handle
            .await
            .unwrap_or_else(|e| Err(format!("HTTP task panicked: {e}")));

        tracing::info!(
            target: LOG,
            action = action.id(),
            request_id = %request_id.label(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "HTTP Lab joined Tokio request task"
        );

        let retry_handle = cx.update(move |cx| {
            tracing::info!(
                target: LOG,
                action = action.id(),
                request_id = %request_id.label(),
                "HTTP Lab applying result on GPUI thread"
            );
            let retry = apply_result(action, request_id, result, cx);
            tracing::info!(
                target: LOG,
                action = action.id(),
                request_id = %request_id.label(),
                "HTTP Lab applied result on GPUI thread"
            );
            retry
        });

        match retry_handle {
            Some(next_handle) => {
                tracing::info!(
                    target: LOG,
                    action = next_handle.action.id(),
                    request_id = %next_handle.request_id.label(),
                    "HTTP Lab retry handle returned, continuing entity task loop"
                );
                current_handle = next_handle;
            }
            None => break,
        }
    }
}

pub struct ActionHandle {
    pub action: HttpLabAction,
    request_id: gpui_query_legacy::RequestId,
    cancellation: tokio_util::sync::CancellationToken,
    http_handle: tokio::task::JoinHandle<Result<Vec<ActionExchange>, String>>,
}

pub fn cancel_action(action: HttpLabAction, cx: &mut App) {
    tracing::info!(target: LOG, action = action.id(), "HTTP Lab cancel requested");
    cx.update_global::<HttpLabState, _>(|state, _cx| {
        cancel_action_in_state(state, action, "Cancelled by user");
    });
}

pub fn cancel_all(cx: &mut App) {
    tracing::info!(target: LOG, "HTTP Lab cancel all requested");
    cx.update_global::<HttpLabState, _>(|state, _cx| {
        cancel_all_in_state(state, "Cancelled by user");
    });
}

/// Prefetch a query: starts a background request without user interaction.
/// Returns `true` if the prefetch was started (a new request was issued),
/// `false` if it was a cache hit or already loading.
pub fn prefetch_action(action: HttpLabAction, cx: &mut App) -> bool {
    let now_ms = now_ms();
    cx.update_global::<HttpLabState, _>(|state, _cx| {
        // Simple prefetch: begin a request on the resource
        let resource = state.resources.get_mut(&action)?;
        match resource.begin_request(&mut state.request_sequencer, now_ms, QueryFetchMode::Normal) {
            QueryBeginResult::Started { request_id, .. } => {
                tracing::info!(target: LOG, action = action.id(), request_id = %request_id.label(), "HTTP Lab prefetch started");
                Some(request_id)
            }
            QueryBeginResult::CacheHit => {
                tracing::info!(target: LOG, action = action.id(), "HTTP Lab prefetch cache hit");
                None
            }
            QueryBeginResult::IgnoredWhileLoading { .. } => {
                tracing::info!(target: LOG, action = action.id(), "HTTP Lab prefetch ignored (already loading)");
                None
            }
        }
    }).is_some()
}

/// Build an `ActionHandle` for a retry attempt, mirroring the setup in
/// `prepare_action` but without the full action-selection side effects.
fn prepare_retry_handle(
    action: HttpLabAction,
    request_id: RequestId,
    cx: &mut App,
) -> Option<ActionHandle> {
    let cancellation = register_request_flag(request_id);
    let rt = cx.global::<TokioRuntimeGlobal>().0.runtime.clone();
    let client = cx.global::<TokioRuntimeGlobal>().0.http_client.clone();
    let request_cancellation = cancellation.clone();
    tracing::info!(
        target: LOG,
        action = action.id(),
        request_id = %request_id.label(),
        "HTTP Lab spawning Tokio retry task"
    );
    let http_handle = rt.spawn(async move {
        tracing::info!(
            target: LOG,
            action = action.id(),
            request_id = %request_id.label(),
            "HTTP Lab Tokio retry task started"
        );
        let result = run_http_action(&client, action, request_cancellation).await;
        match &result {
            Ok(exchanges) => tracing::info!(
                target: LOG,
                action = action.id(),
                request_id = %request_id.label(),
                exchange_count = exchanges.len(),
                "HTTP Lab Tokio retry task completed"
            ),
            Err(error) => tracing::warn!(
                target: LOG,
                action = action.id(),
                request_id = %request_id.label(),
                error = %error,
                "HTTP Lab Tokio retry task failed"
            ),
        }
        result
    });

    Some(ActionHandle {
        action,
        request_id,
        cancellation,
        http_handle,
    })
}

/// Apply a result to state and, if the failure is retryable, return a
/// prepared `ActionHandle` for the retry attempt.
///
/// The caller (the entity task in `execute_action`) awaits the retry handle
/// and calls this function again with the retry's result. This keeps retry
/// wiring on the entity task where `cx.update` is available, rather than
/// trying to spawn from synchronous `&mut App` context.
fn apply_result(
    action: HttpLabAction,
    request_id: RequestId,
    result: Result<Vec<ActionExchange>, String>,
    cx: &mut App,
) -> Option<ActionHandle> {
    let now_ms = now_ms();
    tracing::debug!(
        target: LOG,
        action = action.id(),
        request_id = %request_id.label(),
        "HTTP Lab reducing result into state"
    );
    let is_failure = result.is_err();
    cx.update_global::<HttpLabState, _>(|state, _cx| {
        apply_result_to_state(state, action, request_id, result, now_ms)
    });

    // Check for retry — only on failure, not on success.
    if !is_failure {
        return None;
    }
    let retry_count =
        cx.update_global::<HttpLabState, _>(|state, _cx| should_retry_action(state, action))?;
    let delay_ms = cx.update_global::<HttpLabState, _>(|state, _cx| {
        state
            .resource(action)
            .retry_policy()
            .delay_for_attempt(retry_count)
    });
    tracing::info!(
        target: LOG,
        action = action.id(),
        retry_count,
        delay_ms,
        "HTTP Lab scheduling retry"
    );
    let retry_request_id =
        cx.update_global::<HttpLabState, _>(|state, _cx| prepare_retry(state, action, now_ms))?;
    prepare_retry_handle(action, retry_request_id, cx)
}

fn now_ms() -> u128 {
    static STARTED_AT: OnceLock<Instant> = OnceLock::new();
    STARTED_AT.get_or_init(Instant::now).elapsed().as_millis()
}
