use super::{
    task_tracking::{cancellation_flags, register_request_flag},
    test_support::{error_message, exchange, seed_response},
    transitions::{apply_result_to_state, begin_action, cancel_action_in_state},
    *,
};
use gpui_query_legacy::QueryStatus;

fn state_after_scope_advances(count: usize) -> HttpLabState {
    let mut state = HttpLabState::default();
    for _ in 0..count {
        state.request_sequencer.advance_scope();
    }
    state
}

#[test]
fn stale_result_is_ignored_after_cancellation() {
    let mut state = HttpLabState::default();
    let request = begin_action(&mut state, HttpLabAction::GetJson, 1).expect("request");

    cancel_action_in_state(&mut state, HttpLabAction::GetJson, "test cancel");
    apply_result_to_state(
        &mut state,
        HttpLabAction::GetJson,
        request,
        Ok(vec![(
            HttpLabAction::GetJson,
            exchange("GET JSON", 200, None),
        )]),
        2,
    );

    let resource = state.resource(HttpLabAction::GetJson);
    assert_eq!(resource.status(), QueryStatus::Cancelled);
    assert!(resource.data().is_none());
    assert_eq!(resource.ignored_results(), 1);
}

#[test]
fn cancelled_request_keeps_task_tracking_until_result_arrives() {
    let mut state = state_after_scope_advances(10);
    let request = begin_action(&mut state, HttpLabAction::GetJson, 1).expect("request");
    let cancellation = register_request_flag(request);

    cancel_action_in_state(&mut state, HttpLabAction::GetJson, "test cancel");
    assert!(cancellation.is_cancelled());

    apply_result_to_state(
        &mut state,
        HttpLabAction::GetJson,
        request,
        Ok(vec![(
            HttpLabAction::GetJson,
            exchange("GET JSON", 200, None),
        )]),
        2,
    );

    assert!(cancellation_flags().lock().unwrap().get(&request).is_none());
}

#[test]
fn successful_result_completes_tracked_task() {
    let mut state = state_after_scope_advances(11);
    let request = begin_action(&mut state, HttpLabAction::GetJson, 1).expect("request");
    register_request_flag(request);

    apply_result_to_state(
        &mut state,
        HttpLabAction::GetJson,
        request,
        Ok(vec![(
            HttpLabAction::GetJson,
            exchange("GET JSON", 200, None),
        )]),
        2,
    );

    assert!(cancellation_flags().lock().unwrap().get(&request).is_none());
}

#[test]
fn successful_exchange_updates_only_target_resource() {
    let mut state = HttpLabState::default();
    let request = begin_action(&mut state, HttpLabAction::GetJson, 1).expect("request");

    apply_result_to_state(
        &mut state,
        HttpLabAction::GetJson,
        request,
        Ok(vec![(
            HttpLabAction::GetJson,
            exchange("GET JSON", 200, None),
        )]),
        2,
    );

    assert_eq!(
        state.resource(HttpLabAction::GetJson).status(),
        QueryStatus::Success
    );
    assert!(state.resource(HttpLabAction::GetJson).data().is_some());
    assert_eq!(
        state.resource(HttpLabAction::GetXml).status(),
        QueryStatus::Idle
    );
    assert_eq!(state.history.len(), 1);
}

#[test]
fn failed_exchange_preserves_previous_data() {
    let mut state = HttpLabState::default();
    let previous = exchange("Failure", 500, Some("HTTP 500"));
    seed_response(&mut state, HttpLabAction::Failure, previous, 0);

    let request = begin_action(&mut state, HttpLabAction::Failure, 1).expect("request");
    apply_result_to_state(
        &mut state,
        HttpLabAction::Failure,
        request,
        Ok(vec![(
            HttpLabAction::Failure,
            exchange("Failure", 503, Some("HTTP 503")),
        )]),
        2,
    );

    let resource = state.resource(HttpLabAction::Failure);
    assert_eq!(resource.status(), QueryStatus::Failure);
    assert!(resource.data().is_some());
    assert_eq!(error_message(resource), Some("HTTP 503"));
}
