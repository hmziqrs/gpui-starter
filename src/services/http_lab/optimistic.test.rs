use super::{
    test_support::{exchange, seed_response},
    transitions::{apply_result_to_state, begin_action},
    *,
};
use gpui_query_legacy::QueryStatus;

// Use PostJson (NoCache policy) for all tests to avoid cache-hit short-circuits.

#[test]
fn set_data_stores_previous_and_updates_current() {
    let mut state = HttpLabState::default();
    seed_response(
        &mut state,
        HttpLabAction::PostJson,
        exchange("original", 200, None),
        10_000,
    );

    state.set_action_data(HttpLabAction::PostJson, exchange("optimistic", 200, None));

    let resource = state.resource(HttpLabAction::PostJson);
    assert_eq!(resource.data().unwrap().label, "optimistic");
    assert_eq!(
        state
            .previous_resource_data(HttpLabAction::PostJson)
            .unwrap()
            .label,
        "original"
    );
}

#[test]
fn clear_data_stores_previous_and_clears_current() {
    let mut state = HttpLabState::default();
    seed_response(
        &mut state,
        HttpLabAction::PostJson,
        exchange("original", 200, None),
        10_000,
    );

    state.clear_action_data(HttpLabAction::PostJson);

    let resource = state.resource(HttpLabAction::PostJson);
    assert!(resource.data().is_none());
    assert_eq!(
        state
            .previous_resource_data(HttpLabAction::PostJson)
            .unwrap()
            .label,
        "original"
    );
}

#[test]
fn rollback_restores_previous_data() {
    let mut state = HttpLabState::default();
    seed_response(
        &mut state,
        HttpLabAction::PostJson,
        exchange("original", 200, None),
        10_000,
    );

    state.set_action_data(HttpLabAction::PostJson, exchange("optimistic", 200, None));
    let rolled_back = state.rollback_action_data(HttpLabAction::PostJson);

    assert!(rolled_back);
    let resource = state.resource(HttpLabAction::PostJson);
    assert_eq!(resource.data().unwrap().label, "original");
    assert_eq!(resource.status(), QueryStatus::Success);
}

#[test]
fn rollback_returns_false_without_previous() {
    let mut state = HttpLabState::default();
    // Fresh resource has no previous data.
    let rolled_back = state.rollback_action_data(HttpLabAction::PostJson);
    assert!(!rolled_back);
}

#[test]
fn set_data_does_not_change_status() {
    let mut state = HttpLabState::default();
    seed_response(
        &mut state,
        HttpLabAction::PostJson,
        exchange("original", 200, None),
        10_000,
    );

    // Resource should be in Success state after seeding.
    assert_eq!(
        state.resource(HttpLabAction::PostJson).status(),
        QueryStatus::Success
    );

    state.set_action_data(HttpLabAction::PostJson, exchange("optimistic", 200, None));

    // Status should not have changed.
    assert_eq!(
        state.resource(HttpLabAction::PostJson).status(),
        QueryStatus::Success
    );
}

#[test]
fn complete_success_after_optimistic_update() {
    let mut state = HttpLabState::default();
    seed_response(
        &mut state,
        HttpLabAction::PostJson,
        exchange("original", 200, None),
        10_000,
    );

    // Optimistic update.
    state.set_action_data(HttpLabAction::PostJson, exchange("optimistic", 200, None));

    // Now start a real request and complete it.
    let request = begin_action(&mut state, HttpLabAction::PostJson, 20_000).expect("request");
    apply_result_to_state(
        &mut state,
        HttpLabAction::PostJson,
        request,
        Ok(vec![(
            HttpLabAction::PostJson,
            exchange("server confirmed", 200, None),
        )]),
        20_001,
    );

    let resource = state.resource(HttpLabAction::PostJson);
    assert_eq!(resource.data().unwrap().label, "server confirmed");
    // Previous data should be the optimistic value (set by apply_success overwriting the optimistic data).
    assert_eq!(
        state
            .previous_resource_data(HttpLabAction::PostJson)
            .unwrap()
            .label,
        "optimistic"
    );
}

#[test]
fn rollback_after_failed_mutation() {
    let mut state = HttpLabState::default();
    seed_response(
        &mut state,
        HttpLabAction::PostJson,
        exchange("original", 200, None),
        10_000,
    );

    // Optimistic update.
    state.set_action_data(HttpLabAction::PostJson, exchange("optimistic", 200, None));

    // Start a real request that fails.
    let request = begin_action(&mut state, HttpLabAction::PostJson, 20_000).expect("request");
    apply_result_to_state(
        &mut state,
        HttpLabAction::PostJson,
        request,
        Err("mutation failed".to_string()),
        20_001,
    );

    // Resource is now in Failure state. Roll back to the data before the optimistic update.
    let rolled_back = state.rollback_action_data(HttpLabAction::PostJson);
    assert!(rolled_back);

    let resource = state.resource(HttpLabAction::PostJson);
    assert_eq!(resource.data().unwrap().label, "original");
}

#[test]
fn double_set_data_keeps_latest_previous() {
    let mut state = HttpLabState::default();
    seed_response(
        &mut state,
        HttpLabAction::PostJson,
        exchange("original", 200, None),
        10_000,
    );

    state.set_action_data(HttpLabAction::PostJson, exchange("first_opt", 200, None));
    state.set_action_data(HttpLabAction::PostJson, exchange("second_opt", 200, None));

    let resource = state.resource(HttpLabAction::PostJson);
    assert_eq!(resource.data().unwrap().label, "second_opt");
    assert_eq!(
        state
            .previous_resource_data(HttpLabAction::PostJson)
            .unwrap()
            .label,
        "first_opt"
    );
}
