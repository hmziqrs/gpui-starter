use super::*;

#[test]
fn test_query_key_construction() {
    let key = HttpLabAction::GetText.query_key();
    assert_eq!(key.as_str(), "http_lab");
    assert_eq!(key.parts().len(), 2);
}

#[test]
fn test_all_actions_have_policies() {
    for action in HttpLabAction::all() {
        let key = action.query_key();
        assert!(!key.parts().is_empty(), "{:?} should have a key", action);
        let _cache = action.cache_policy();
        let _request = action.request_policy();
        let _retry = action.retry_policy();
    }
}

#[test]
fn test_state_resource_retry_policy_integration() {
    let state = HttpLabState::default();
    // GET resources should have retry
    assert!(
        state
            .resource(HttpLabAction::GetText)
            .retry_policy()
            .max_retries
            > 0
    );
    assert!(
        state
            .resource(HttpLabAction::GetJson)
            .retry_policy()
            .max_retries
            > 0
    );
    assert!(
        state
            .resource(HttpLabAction::GetXml)
            .retry_policy()
            .max_retries
            > 0
    );
    // POST resources should not
    assert_eq!(
        state
            .resource(HttpLabAction::PostJson)
            .retry_policy()
            .max_retries,
        0
    );
    assert_eq!(
        state
            .resource(HttpLabAction::PostForm)
            .retry_policy()
            .max_retries,
        0
    );
}

#[test]
fn test_state_reset_preserves_policies() {
    let mut state = HttpLabState::default();
    let policy = state
        .resource(HttpLabAction::GetText)
        .retry_policy()
        .clone();

    let _reset = state.reset_for_user();

    // After reset, new resources should have same policies
    let new_policy = state.resource(HttpLabAction::GetText).retry_policy();
    assert_eq!(policy.max_retries, new_policy.max_retries);
    assert_eq!(policy.retry_delay_ms, new_policy.retry_delay_ms);
}
