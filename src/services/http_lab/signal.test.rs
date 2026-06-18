use super::{transitions::begin_action, *};
use gpui_query_legacy::QueryError;

#[test]
fn signal_is_created_on_begin_request() {
    let mut state = HttpLabState::default();
    let request = begin_action(&mut state, HttpLabAction::GetText, 1);
    assert!(request.is_some());

    let signal = state.signal_for_action(HttpLabAction::GetText);
    assert!(signal.is_some());
    assert!(!signal.unwrap().is_cancelled());
}

#[test]
fn signal_is_cancelled_on_resource_cancel() {
    let mut state = HttpLabState::default();
    begin_action(&mut state, HttpLabAction::GetText, 1);

    let signal = state
        .signal_for_action(HttpLabAction::GetText)
        .expect("signal");
    assert!(!signal.is_cancelled());

    // Cancel the resource.
    let resource = state.resources.get_mut(&HttpLabAction::GetText).unwrap();
    resource.cancel(QueryError::cancelled("test cancel"));

    // The cloned signal should now read cancelled.
    assert!(signal.is_cancelled());
}

#[test]
fn signal_is_absent_on_idle_resource() {
    let state = HttpLabState::default();

    let signal = state.signal_for_action(HttpLabAction::GetText);
    assert!(signal.is_none());
}

#[test]
fn signal_is_fresh_on_second_request_after_cancel() {
    let mut state = HttpLabState::default();
    begin_action(&mut state, HttpLabAction::GetText, 1);

    // Cancel the first request.
    let resource = state.resources.get_mut(&HttpLabAction::GetText).unwrap();
    resource.cancel(QueryError::cancelled("cancelled"));

    // Begin a new request.
    begin_action(&mut state, HttpLabAction::GetText, 2);

    let signal = state
        .signal_for_action(HttpLabAction::GetText)
        .expect("signal");
    assert!(!signal.is_cancelled());
}

#[test]
fn signal_clones_share_cancellation() {
    let mut state = HttpLabState::default();
    begin_action(&mut state, HttpLabAction::GetJson, 1);

    let signal1 = state
        .signal_for_action(HttpLabAction::GetJson)
        .expect("signal");
    let signal2 = state
        .signal_for_action(HttpLabAction::GetJson)
        .expect("signal");

    assert!(!signal1.is_cancelled());
    assert!(!signal2.is_cancelled());

    // Cancel via the resource.
    let resource = state.resources.get_mut(&HttpLabAction::GetJson).unwrap();
    resource.cancel(QueryError::cancelled("shared cancel"));

    // Both clones should see the cancellation.
    assert!(signal1.is_cancelled());
    assert!(signal2.is_cancelled());
}
