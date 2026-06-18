use super::{
    state::ResetRequests,
    test_support::exchange,
    transitions::{apply_result_to_state, begin_action},
    *,
};
use gpui_query_legacy::QueryStatus;

#[test]
fn full_flow_populates_individual_resources_and_flow_resource() {
    let mut state = HttpLabState::default();
    let request = begin_action(&mut state, HttpLabAction::FullFlow, 1).expect("flow request");
    let exchanges = vec![
        (HttpLabAction::GetJson, exchange("GET JSON", 200, None)),
        (
            HttpLabAction::Failure,
            exchange("Failure", 503, Some("HTTP 503")),
        ),
        (HttpLabAction::PostJson, exchange("POST JSON", 200, None)),
    ];

    apply_result_to_state(
        &mut state,
        HttpLabAction::FullFlow,
        request,
        Ok(exchanges),
        2,
    );

    assert_eq!(
        state.resource(HttpLabAction::FullFlow).status(),
        QueryStatus::Success
    );
    assert_eq!(
        state.resource(HttpLabAction::GetJson).status(),
        QueryStatus::Success
    );
    assert_eq!(
        state.resource(HttpLabAction::Failure).status(),
        QueryStatus::Failure
    );
    assert_eq!(
        state.resource(HttpLabAction::PostJson).status(),
        QueryStatus::Success
    );
    assert_eq!(state.history.len(), 3);
}

#[test]
fn individual_request_cancels_pending_full_flow_before_it_can_apply_results() {
    let mut state = HttpLabState::default();
    let flow_request = begin_action(&mut state, HttpLabAction::FullFlow, 1).expect("flow request");
    let individual_request =
        begin_action(&mut state, HttpLabAction::GetJson, 2).expect("individual request");

    apply_result_to_state(
        &mut state,
        HttpLabAction::FullFlow,
        flow_request,
        Ok(vec![(
            HttpLabAction::GetJson,
            exchange("GET JSON from stale flow", 200, None),
        )]),
        3,
    );

    assert_eq!(
        state.resource(HttpLabAction::FullFlow).status(),
        QueryStatus::Cancelled
    );
    assert_eq!(state.resource(HttpLabAction::FullFlow).ignored_results(), 1);
    assert_eq!(
        state.resource(HttpLabAction::GetJson).active_request_id(),
        Some(individual_request)
    );
    assert!(state.resource(HttpLabAction::GetJson).data().is_none());
}

#[test]
fn reset_makes_in_flight_request_results_silent_stale_results() {
    let mut state = HttpLabState::default();
    let request = begin_action(&mut state, HttpLabAction::GetJson, 1).expect("request");

    let reset_requests = state.reset_for_user();
    let transition_log = state.transition_log.clone();
    apply_result_to_state(
        &mut state,
        HttpLabAction::GetJson,
        request,
        Ok(vec![(
            HttpLabAction::GetJson,
            exchange("GET JSON from stale scope", 200, None),
        )]),
        2,
    );

    let resource = state.resource(HttpLabAction::GetJson);
    assert_eq!(resource.status(), QueryStatus::Idle);
    assert_eq!(resource.ignored_results(), 0);
    assert_eq!(state.transition_log, transition_log);
    assert_eq!(
        reset_requests,
        ResetRequests {
            request_ids: vec![request],
        }
    );
}

#[test]
fn reset_for_user_returns_only_request_ids() {
    let mut state = HttpLabState::default();
    let request = begin_action(&mut state, HttpLabAction::GetJson, 1).expect("request");

    let reset_requests = state.reset_for_user();

    assert_eq!(reset_requests.request_ids, vec![request]);
    assert_eq!(
        state.resource(HttpLabAction::GetJson).status(),
        QueryStatus::Idle
    );
}

#[test]
fn reset_prevents_old_request_id_from_colliding_with_new_request() {
    let mut state = HttpLabState::default();
    let old_request = begin_action(&mut state, HttpLabAction::GetJson, 1).expect("old request");

    let _reset_requests = state.reset_for_user();
    let new_request = begin_action(&mut state, HttpLabAction::GetJson, 2).expect("new request");
    apply_result_to_state(
        &mut state,
        HttpLabAction::GetJson,
        old_request,
        Ok(vec![(
            HttpLabAction::GetJson,
            exchange("GET JSON from stale scope", 200, None),
        )]),
        3,
    );

    let resource = state.resource(HttpLabAction::GetJson);
    assert_eq!(old_request.value(), new_request.value());
    assert_ne!(old_request, new_request);
    assert_eq!(resource.active_request_id(), Some(new_request));
    assert!(resource.data().is_none());
}

#[test]
fn cookies_exchange_updates_cookie_snapshot() {
    let mut cookies = exchange("Cookies", 200, None);
    let response = cookies.response.as_mut().expect("response");
    response
        .headers
        .push(("set-cookie".to_string(), "session=gpui-starter".to_string()));
    response.parsed_json = Some("{\"cookies\":{\"session\":\"gpui-starter\"}}".to_string());
    let mut state = HttpLabState::default();
    let request = begin_action(&mut state, HttpLabAction::Cookies, 1).expect("request");

    apply_result_to_state(
        &mut state,
        HttpLabAction::Cookies,
        request,
        Ok(vec![(HttpLabAction::Cookies, cookies)]),
        2,
    );

    let cookies = state.cookies.expect("cookie snapshot");
    assert_eq!(
        cookies.set_cookie_header.as_deref(),
        Some("session=gpui-starter")
    );
    assert_eq!(
        cookies.echoed_cookies_json.as_deref(),
        Some("{\"cookies\":{\"session\":\"gpui-starter\"}}")
    );
}
