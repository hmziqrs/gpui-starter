use gpui_query::{
    CachePolicy, QueryBeginResult, QueryError, QueryFetchMode, QueryStatus, RequestGuard,
    RequestId,
};

use crate::services::http_lab::{
    client::HTTP_LAB_BASE,
    response::cookie_snapshot_from_exchange,
    state::HttpLabState,
    task_tracking::{cancel_request_flag, remove_request_flag},
    types::{
        ActionExchange, HttpExchange, HttpLabAction, HttpRequestBodyKind, HttpRequestSnapshot,
    },
};

const LOG: &str = "gpui_starter::http_lab::state";

pub(super) fn begin_action(
    state: &mut HttpLabState,
    action: HttpLabAction,
    now_ms: u128,
) -> Option<RequestId> {
    tracing::debug!(
        target: LOG,
        action = action.id(),
        now_ms,
        "HTTP Lab begin_action entered"
    );
    state.selected_action = action;

    if action == HttpLabAction::FullFlow {
        cancel_all_in_state(state, "Cancelled by full flow");
    } else {
        cancel_action_in_state(
            state,
            HttpLabAction::FullFlow,
            "Cancelled by individual request",
        );
    }

    let has_data = state.resource(action).has_data();
    let cache_policy = state.resource(action).cache_policy();
    let begin_result = {
        let HttpLabState {
            resources,
            request_sequencer,
            ..
        } = state;
        let resource = resources.get_mut(&action)?;
        resource.begin_request(request_sequencer, now_ms, QueryFetchMode::Normal)
    };

    match begin_result {
        QueryBeginResult::Started {
            request_id,
            status,
            replaced_request_id,
        } => {
            if replaced_request_id.is_some() {
                record_transition(
                    state,
                    QueryStatus::Cancelled,
                    action.label(),
                    "cancelled by newer request",
                );
            }
            let note = match (cache_policy, has_data) {
                (CachePolicy::StaleWhileRevalidate { .. }, true) => "revalidating cached data",
                _ => "request started",
            };
            record_transition(state, status, action.label(), note);
            tracing::info!(
                target: LOG,
                action = action.id(),
                request_id = %request_id.label(),
                status = status.label(),
                note,
                "HTTP Lab request state started"
            );
            Some(request_id)
        }
        QueryBeginResult::CacheHit => {
            record_transition(state, QueryStatus::Success, action.label(), "cache hit");
            tracing::info!(
                target: LOG,
                action = action.id(),
                "HTTP Lab request short-circuited by cache"
            );
            None
        }
        QueryBeginResult::IgnoredWhileLoading { .. } => {
            record_transition(
                state,
                state.resource(action).status(),
                action.label(),
                "ignored duplicate while loading",
            );
            tracing::info!(
                target: LOG,
                action = action.id(),
                "HTTP Lab request ignored while loading"
            );
            None
        }
    }
}

pub(super) fn apply_result_to_state(
    state: &mut HttpLabState,
    action: HttpLabAction,
    request_id: RequestId,
    result: Result<Vec<ActionExchange>, String>,
    now_ms: u128,
) {
    tracing::debug!(
        target: LOG,
        action = action.id(),
        request_id = %request_id.label(),
        ok = result.is_ok(),
        "HTTP Lab apply_result_to_state entered"
    );
    remove_request_flag(request_id);
    if !state.request_sequencer.is_current_scope(request_id) {
        tracing::warn!(
            target: LOG,
            action = action.id(),
            request_id = %request_id.label(),
            "HTTP Lab result ignored because request scope is stale"
        );
        return;
    }

    let Some(request_guard) = state
        .resources
        .get_mut(&action)
        .and_then(|resource| resource.accept_current_request(request_id))
    else {
        record_ignored_result(state, action, request_id);
        tracing::warn!(
            target: LOG,
            action = action.id(),
            request_id = %request_id.label(),
            "HTTP Lab result ignored because request is no longer current"
        );
        return;
    };

    match result {
        Ok(exchanges) => {
            tracing::info!(
                target: LOG,
                action = action.id(),
                request_id = %request_id.label(),
                exchange_count = exchanges.len(),
                "HTTP Lab applying successful exchanges"
            );
            let last_exchange = exchanges.last().map(|(_, exchange)| exchange.clone());
            for (index, (target_action, exchange)) in exchanges.into_iter().enumerate() {
                if action == HttpLabAction::FullFlow && index > 0 {
                    record_transition(
                        state,
                        QueryStatus::LoadingWithData,
                        target_action.label(),
                        "full flow advanced",
                    );
                }
                finish_exchange(state, target_action, exchange, &request_guard, now_ms);
            }

            if action == HttpLabAction::FullFlow {
                finish_flow_resource(state, last_exchange, &request_guard, now_ms);
            }
        }
        Err(error) => {
            tracing::warn!(
                target: LOG,
                action = action.id(),
                request_id = %request_id.label(),
                error = %error,
                "HTTP Lab applying failed exchange"
            );
            fail_resource(state, action, &request_guard, error.clone());
        }
    }
}

/// Check if the action should be retried after a failure.
/// Returns the current retry count if a retry is allowed, `None` otherwise.
pub(super) fn should_retry_action(state: &HttpLabState, action: HttpLabAction) -> Option<u32> {
    let resource = state.resource(action);
    if resource.retry_policy().should_retry(resource.retry_count()) {
        Some(resource.retry_count())
    } else {
        None
    }
}

/// Prepare a resource for retry by incrementing its retry counter and starting
/// a new request in `Force` mode (bypassing cache). Returns the new `RequestId`
/// if the retry was started, or `None` if retries are exhausted.
pub(super) fn prepare_retry(
    state: &mut HttpLabState,
    action: HttpLabAction,
    now_ms: u128,
) -> Option<RequestId> {
    let resource = state.resources.get_mut(&action)?;
    if !resource.retry_policy().should_retry(resource.retry_count()) {
        return None;
    }
    resource.increment_retry();

    let begin_result = {
        let HttpLabState {
            resources,
            request_sequencer,
            ..
        } = state;
        let resource = resources.get_mut(&action)?;
        resource.begin_request(request_sequencer, now_ms, QueryFetchMode::Force)
    };

    match begin_result {
        QueryBeginResult::Started { request_id, .. } => {
            let resource = state.resource(action);
            let retry_count = resource.retry_count();
            let delay_ms = resource.retry_policy().delay_for_attempt(retry_count - 1);
            record_transition(
                state,
                QueryStatus::LoadingWithData,
                action.label(),
                &format!("retry attempt {} (delay {}ms)", retry_count, delay_ms),
            );
            tracing::info!(
                target: LOG,
                action = action.id(),
                retry_count,
                "HTTP Lab retry request started"
            );
            Some(request_id)
        }
        _ => None,
    }
}

fn finish_exchange(
    state: &mut HttpLabState,
    action: HttpLabAction,
    exchange: HttpExchange,
    request_guard: &RequestGuard,
    now_ms: u128,
) {
    tracing::info!(
        target: LOG,
        action = action.id(),
        status = if exchange.error.is_none() { "success" } else { "failure" },
        "HTTP Lab finish_exchange"
    );
    let status = if exchange.error.is_none() {
        QueryStatus::Success
    } else {
        QueryStatus::Failure
    };
    let error = exchange.error.clone();

    if let Some(resource) = state.resources.get_mut(&action) {
        match error {
            Some(error) => {
                resource.complete_failure_with_data(
                    request_guard,
                    exchange.clone(),
                    QueryError::response(error),
                );
            }
            None => resource.complete_success(request_guard, exchange.clone(), now_ms),
        }
    }

    if let Some(cookie_snapshot) = cookie_snapshot_from_exchange(&exchange) {
        state.cookies = Some(cookie_snapshot);
    }

    record_transition(state, status, action.label(), "response applied");
    push_history(state, exchange);
}

fn finish_flow_resource(
    state: &mut HttpLabState,
    last_exchange: Option<HttpExchange>,
    request_guard: &RequestGuard,
    now_ms: u128,
) {
    if let Some(resource) = state.resources.get_mut(&HttpLabAction::FullFlow) {
        resource.complete_success_optional(request_guard, last_exchange, now_ms);
    }
    record_transition(
        state,
        QueryStatus::Success,
        HttpLabAction::FullFlow.label(),
        "flow completed",
    );
}

fn fail_resource(
    state: &mut HttpLabState,
    action: HttpLabAction,
    request_guard: &RequestGuard,
    error: String,
) {
    let exchange = HttpExchange {
        label: action.label().to_string(),
        request: HttpRequestSnapshot {
            method: "-".to_string(),
            url: HTTP_LAB_BASE.to_string(),
            request_body_kind: HttpRequestBodyKind::None,
            request_body_preview: String::new(),
        },
        response: None,
        error: Some(error.clone()),
    };

    if let Some(resource) = state.resources.get_mut(&action) {
        resource.complete_failure(request_guard, QueryError::transport(error));
    }

    record_transition(
        state,
        QueryStatus::Failure,
        action.label(),
        "request failed",
    );
    push_history(state, exchange);
}

pub(super) fn cancel_action_in_state(
    state: &mut HttpLabState,
    action: HttpLabAction,
    reason: &str,
) {
    if let Some(request_id) = state.resource(action).active_request_id() {
        tracing::info!(
            target: LOG,
            action = action.id(),
            request_id = %request_id.label(),
            reason,
            "HTTP Lab cancelling action in state"
        );
        cancel_request_flag(request_id);
        if let Some(resource) = state.resources.get_mut(&action) {
            resource.cancel(QueryError::cancelled(reason));
        }
        record_transition(state, QueryStatus::Cancelled, action.label(), reason);
    }
}

pub(super) fn cancel_all_in_state(state: &mut HttpLabState, reason: &str) {
    let actions = HttpLabAction::all()
        .iter()
        .copied()
        .filter(|action| state.resource(*action).active_request_id().is_some())
        .collect::<Vec<_>>();
    for action in actions {
        cancel_action_in_state(state, action, reason);
    }
}

fn record_ignored_result(state: &mut HttpLabState, action: HttpLabAction, request_id: RequestId) {
    record_transition(
        state,
        QueryStatus::Cancelled,
        action.label(),
        &format!("ignored stale request {}", request_id.label()),
    );
}

fn record_transition(state: &mut HttpLabState, status: QueryStatus, label: &str, note: &str) {
    state
        .transition_log
        .insert(0, format!("{}: {label} ({note})", status.label()));
    state.transition_log.truncate(24);
}

fn push_history(state: &mut HttpLabState, exchange: HttpExchange) {
    state.history.insert(0, exchange);
    state.history.truncate(24);
}
