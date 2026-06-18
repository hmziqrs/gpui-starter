use super::{
    test_support::{exchange, seed_response},
    transitions::begin_action,
    *,
};
use gpui_query_legacy::QueryStatus;

#[test]
fn starts_each_action_with_its_own_resource() {
    let state = HttpLabState::default();

    for action in HttpLabAction::all() {
        let resource = state.resource(*action);
        assert_eq!(resource.key(), &action.query_key());
        assert_eq!(resource.status(), QueryStatus::Idle);
    }
}

#[test]
fn ttl_cache_can_short_circuit_request() {
    let mut state = HttpLabState::default();
    let now_ms = 10_000;
    seed_response(
        &mut state,
        HttpLabAction::GetText,
        exchange("GET text", 200, None),
        now_ms - 100,
    );

    let request = begin_action(&mut state, HttpLabAction::GetText, now_ms);

    assert!(request.is_none());
    let resource = state.resource(HttpLabAction::GetText);
    assert_eq!(resource.status(), QueryStatus::Success);
    assert_eq!(resource.cache_hits(), 1);
}

#[test]
fn stale_while_revalidate_keeps_data_and_starts_request() {
    let mut state = HttpLabState::default();
    let now_ms = 100_000;
    seed_response(
        &mut state,
        HttpLabAction::GetJson,
        exchange("GET JSON", 200, None),
        now_ms - 1,
    );

    let request = begin_action(&mut state, HttpLabAction::GetJson, now_ms);

    assert!(request.is_some());
    let resource = state.resource(HttpLabAction::GetJson);
    assert_eq!(resource.status(), QueryStatus::LoadingWithData);
    assert!(resource.data().is_some());
}

#[test]
fn latest_wins_cancels_previous_request_for_same_action() {
    let mut state = HttpLabState::default();
    let first = begin_action(&mut state, HttpLabAction::GetXml, 1).expect("first request");
    let second = begin_action(&mut state, HttpLabAction::GetXml, 2).expect("second request");

    assert_ne!(first, second);
    assert_eq!(state.active_count(), 1);
    assert_eq!(
        state.resource(HttpLabAction::GetXml).active_request_id(),
        Some(second)
    );
    assert_eq!(state.resource(HttpLabAction::GetXml).cancelled_count(), 1);
}

#[test]
fn ignore_while_loading_policy_does_not_start_duplicate_request() {
    let mut state = HttpLabState::default();
    let first = begin_action(&mut state, HttpLabAction::PostMultipart, 1).expect("first request");
    let duplicate = begin_action(&mut state, HttpLabAction::PostMultipart, 2);

    assert!(duplicate.is_none());
    assert_eq!(
        state
            .resource(HttpLabAction::PostMultipart)
            .active_request_id(),
        Some(first)
    );
    assert_eq!(state.active_count(), 1);
}
