use super::{
    test_support::{exchange, seed_response},
    transitions::begin_action,
    *,
};
use gpui_query_legacy::QueryStatus;

#[test]
fn display_data_returns_data_when_present() {
    let mut state = HttpLabState::default();
    seed_response(
        &mut state,
        HttpLabAction::PostJson,
        exchange("POST JSON", 200, None),
        10_000,
    );

    let displayed = state.display_resource(HttpLabAction::PostJson);
    assert!(displayed.is_some());
    assert_eq!(displayed.unwrap().label, "POST JSON");
}

#[test]
fn display_data_returns_placeholder_when_no_data() {
    let mut state = HttpLabState::default();
    state.set_placeholder_for_action(
        HttpLabAction::PostJson,
        Some(exchange("placeholder", 0, None)),
    );

    // Resource is Idle with no real data, but placeholder is set.
    let resource = state.resource(HttpLabAction::PostJson);
    assert_eq!(resource.status(), QueryStatus::Idle);
    assert!(resource.data().is_none());

    // display_resource falls back to placeholder.
    let displayed = state.display_resource(HttpLabAction::PostJson);
    assert!(displayed.is_some());
    assert_eq!(displayed.unwrap().label, "placeholder");
}

#[test]
fn placeholder_data_visible_during_loading() {
    let mut state = HttpLabState::default();

    // Reset the resource first to clear data.
    state
        .resources
        .get_mut(&HttpLabAction::PostJson)
        .unwrap()
        .reset();

    // THEN set the placeholder (after reset, since reset clears placeholder_data).
    state.set_placeholder_for_action(
        HttpLabAction::PostJson,
        Some(exchange("placeholder", 0, None)),
    );

    // Begin a new request.
    let request = begin_action(&mut state, HttpLabAction::PostJson, 20_000);
    assert!(request.is_some());

    // During loading, display_data returns the placeholder.
    let resource = state.resource(HttpLabAction::PostJson);
    assert!(resource.is_loading());
    assert!(resource.data().is_none());
    let displayed = state.display_resource(HttpLabAction::PostJson);
    assert!(displayed.is_some());
    assert_eq!(displayed.unwrap().label, "placeholder");
}

#[test]
fn success_stores_previous_data() {
    let mut state = HttpLabState::default();

    // Use PostJson (NoCache) so second seed doesn't hit cache.
    seed_response(
        &mut state,
        HttpLabAction::PostJson,
        exchange("first", 200, None),
        10_000,
    );

    // Second seed pushes "first" into previous_data.
    seed_response(
        &mut state,
        HttpLabAction::PostJson,
        exchange("second", 200, None),
        20_000,
    );

    let resource = state.resource(HttpLabAction::PostJson);
    assert_eq!(resource.data().unwrap().label, "second");

    let previous = state.previous_resource_data(HttpLabAction::PostJson);
    assert!(previous.is_some());
    assert_eq!(previous.unwrap().label, "first");
}

#[test]
fn rollback_restores_previous_data() {
    let mut state = HttpLabState::default();

    // Use PostJson (NoCache) so second seed doesn't hit cache.
    seed_response(
        &mut state,
        HttpLabAction::PostJson,
        exchange("original", 200, None),
        10_000,
    );
    seed_response(
        &mut state,
        HttpLabAction::PostJson,
        exchange("updated", 200, None),
        20_000,
    );

    // Rollback should restore "original".
    let rolled_back = state.rollback_action_data(HttpLabAction::PostJson);
    assert!(rolled_back);

    let resource = state.resource(HttpLabAction::PostJson);
    assert_eq!(resource.data().unwrap().label, "original");
    assert_eq!(resource.status(), QueryStatus::Success);
}
