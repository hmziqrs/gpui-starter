use super::*;
use gpui_query_legacy::*;

#[test]
fn test_action_retry_policies() {
    // Verify GET actions have retry policies
    let get_text_policy = HttpLabAction::GetText.retry_policy();
    assert!(get_text_policy.should_retry(0));
    assert!(get_text_policy.should_retry(2));
    assert!(!get_text_policy.should_retry(3)); // max is 3

    // Verify POST actions have no retry
    let post_policy = HttpLabAction::PostJson.retry_policy();
    assert!(!post_policy.should_retry(0));

    // Verify Failure has 2 retries
    let fail_policy = HttpLabAction::Failure.retry_policy();
    assert!(fail_policy.should_retry(0));
    assert!(fail_policy.should_retry(1));
    assert!(!fail_policy.should_retry(2));
}

#[test]
fn test_resource_has_retry_policy_set() {
    let state = HttpLabState::default();
    let resource = state.resource(HttpLabAction::GetText);
    assert!(resource.retry_policy().should_retry(0));

    let post_resource = state.resource(HttpLabAction::PostJson);
    assert!(!post_resource.retry_policy().should_retry(0));
}

#[test]
fn test_retry_count_increments_on_failure() {
    let mut resource = QueryResource::<String>::new(
        QueryKey::from_single("test"),
        CachePolicy::NoCache,
        RequestPolicy::LatestWins,
    );
    resource.set_retry_policy(RetryPolicy::new(3).with_delay(100));

    let mut sequencer = RequestSequencer::new();
    let now = 1000u128;

    // Begin and fail first attempt
    let rid = match resource.begin_request(&mut sequencer, now, QueryFetchMode::Normal) {
        QueryBeginResult::Started { request_id, .. } => request_id,
        _ => panic!("expected started"),
    };
    let guard = resource.accept_current_request(rid).unwrap();
    resource.complete_failure(&guard, QueryError::transport("timeout"));

    assert_eq!(resource.retry_count(), 0); // retry_count tracks retries, not failures
    // Increment retry for next attempt
    resource.increment_retry();
    assert_eq!(resource.retry_count(), 1);
}

#[test]
fn test_exponential_backoff_delay() {
    let policy = HttpLabAction::GetText.retry_policy();
    assert!(policy.exponential_backoff);
    let d0 = policy.delay_for_attempt(0);
    let d1 = policy.delay_for_attempt(1);
    let d2 = policy.delay_for_attempt(2);
    assert!(d1 > d0, "exponential: d1 ({d1}) should be > d0 ({d0})");
    assert!(d2 > d1, "exponential: d2 ({d2}) should be > d1 ({d1})");
    assert!(d2 <= 10_000, "should be capped at max_delay");
}
