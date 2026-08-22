//! Basic lifecycle, reset, latest-wins, data preservation, key, and misc tests.

use crate::core::*;

// -- New mutation is idle --

#[test]
fn new_mutation_is_idle() {
    let m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    assert!(m.is_idle());
    assert!(!m.is_loading());
    assert!(!m.is_success());
    assert!(!m.is_failure());
    assert_eq!(m.status(), MutationStatus::Idle);
    assert!(m.data().is_none());
    assert!(m.error().is_none());
    assert!(m.variables().is_none());
    assert_eq!(m.retry_count(), 0);
    assert_eq!(m.cancelled_count(), 0);
    assert!(m.signal().is_none());
    assert!(m.key().is_none());
}

// -- Full lifecycle: Idle -> Loading -> Success --

#[test]
fn lifecycle_idle_to_loading_to_success() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());

    // Idle
    assert_eq!(m.status(), MutationStatus::Idle);

    // Begin -> Loading
    m.begin("update-user".to_string());
    assert!(m.is_loading());
    assert_eq!(m.variables(), Some(&"update-user".to_string()));
    assert!(m.error().is_none());
    assert!(m.signal().is_some());
    assert!(!m.signal().unwrap().is_cancelled());

    // Complete with data -> Success
    m.complete_success(42);
    assert!(m.is_success());
    assert_eq!(m.data(), Some(&42));
    assert!(m.error().is_none());
    assert!(m.signal().is_none());
    // Variables persist through success
    assert_eq!(m.variables(), Some(&"update-user".to_string()));
}

// -- Full lifecycle: Idle -> Loading -> Failure --

#[test]
fn lifecycle_idle_to_loading_to_failure() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());

    m.begin("delete-user".to_string());
    assert!(m.is_loading());

    m.complete_failure(QueryError::response("server error"));
    assert!(m.is_failure());
    assert_eq!(m.error().unwrap().message(), "server error");
    assert_eq!(m.error().unwrap().kind(), QueryErrorKind::Response);
    assert!(m.data().is_none(), "failure must clear previous data");
    assert!(m.signal().is_none());
    assert_eq!(m.retry_count(), 1);
    assert_eq!(m.variables(), Some(&"delete-user".to_string()));
}

// -- Reset returns to Idle, clears everything --

#[test]
fn reset_returns_to_idle_and_clears_all_state() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(3));

    // Build up some state: load -> fail -> retry -> cancel
    m.begin("vars".to_string());
    m.complete_failure(QueryError::response("fail"));
    assert!(m.retry());
    m.cancel(QueryError::cancelled("abort"));

    assert_eq!(m.cancelled_count(), 1);
    assert!(m.is_failure());

    m.reset();

    assert!(m.is_idle());
    assert!(m.data().is_none());
    assert!(m.error().is_none());
    assert!(m.variables().is_none());
    assert_eq!(m.retry_count(), 0);
    assert_eq!(m.cancelled_count(), 0, "reset clears cancelled_count");
    assert!(m.signal().is_none());
}

// -- Reset cancels in-flight signal --

#[test]
fn reset_cancels_in_flight_signal() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    m.begin("vars".to_string());
    let signal = m.signal().unwrap().clone();
    assert!(!signal.is_cancelled());

    m.reset();

    assert!(signal.is_cancelled(), "reset must cancel the in-flight signal");
    assert!(m.signal().is_none());
}

// -- LatestWins: begin cancels previous in-flight signal --

#[test]
fn begin_cancels_previous_signal_on_replacement() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());

    m.begin("first".to_string());
    let first_signal = m.signal().unwrap().clone();
    assert!(!first_signal.is_cancelled());

    // Second begin while first is still loading -> LatestWins
    m.begin("second".to_string());
    assert!(first_signal.is_cancelled(), "old signal must be cancelled");
    assert!(!m.signal().unwrap().is_cancelled(), "new signal must be fresh");
    assert_eq!(m.variables(), Some(&"second".to_string()));
    assert_eq!(m.retry_count(), 0, "begin resets retry_count");
}

// -- LatestWins: data from previous success is overwritten --

#[test]
fn begin_clears_error_from_previous_failure() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());

    m.begin("first".to_string());
    m.complete_failure(QueryError::response("first failed"));
    assert!(m.is_failure());
    assert!(m.error().is_some());

    m.begin("second".to_string());
    assert!(m.is_loading());
    assert!(m.error().is_none(), "begin clears previous error");
}

// -- Data preservation: success data persists until failure --

#[test]
fn success_data_preserved_until_next_failure() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(2));

    // First invocation succeeds
    m.begin("v1".to_string());
    m.complete_success(100);
    assert_eq!(m.data(), Some(&100));

    // Second invocation fails -- data is cleared
    m.begin("v2".to_string());
    m.complete_failure(QueryError::response("fail"));
    assert!(m.data().is_none(), "failure clears data");

    // Third invocation succeeds again -- new data
    m.begin("v3".to_string());
    m.complete_success(200);
    assert_eq!(m.data(), Some(&200));
}

// -- begin resets retry_count for fresh invocation --

#[test]
fn begin_resets_retry_count_allowing_fresh_retries() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(1));

    // First invocation: exhaust retries
    m.begin("v1".to_string());
    m.complete_failure(QueryError::response("fail 1"));
    assert_eq!(m.retry_count(), 1);
    assert!(!m.should_retry(), "retries exhausted");

    // Second invocation: begin resets retry_count
    m.begin("v2".to_string());
    assert_eq!(m.retry_count(), 0, "begin must reset retry_count");
    assert!(m.should_retry(), "should_retry is true after fresh begin");

    m.complete_failure(QueryError::response("fail 2"));
    assert_eq!(m.retry_count(), 1);
    assert!(!m.should_retry(), "retries exhausted again");
}

// -- Mutation with key association --

#[test]
fn mutation_with_key_association() {
    let m: MutationResource<String, i32> =
        MutationResource::new(RetryPolicy::no_retries()).with_key(QueryKey::from(["users", "42"]));

    assert!(m.key().is_some());
    assert_eq!(m.key().unwrap(), &QueryKey::from(["users", "42"]));
}

#[test]
fn mutation_without_key() {
    let m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    assert!(m.key().is_none());
}

// -- Status labels --

#[test]
fn status_labels() {
    assert_eq!(MutationStatus::Idle.label(), "Idle");
    assert_eq!(MutationStatus::Loading.label(), "Loading");
    assert_eq!(MutationStatus::Success.label(), "Success");
    assert_eq!(MutationStatus::Failure.label(), "Failure");
}

// -- retry_policy accessor returns the configured policy --

#[test]
fn retry_policy_accessor() {
    let policy = RetryPolicy::new(5).with_delay(200).with_exponential_backoff();
    let m: MutationResource<String, i32> = MutationResource::new(policy.clone());
    assert_eq!(m.retry_policy(), &policy);
    assert_eq!(m.retry_policy().max_retries, 5);
}

// -- Multiple sequential mutations work correctly --

#[test]
fn multiple_sequential_mutations() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());

    // First mutation
    m.begin("a".to_string());
    m.complete_success(1);
    assert_eq!(m.data(), Some(&1));
    assert_eq!(m.variables(), Some(&"a".to_string()));

    // Second mutation
    m.begin("b".to_string());
    assert_eq!(m.retry_count(), 0, "retry_count reset on new begin");
    m.complete_success(2);
    assert_eq!(m.data(), Some(&2));
    assert_eq!(m.variables(), Some(&"b".to_string()));

    // Third mutation fails
    m.begin("c".to_string());
    m.complete_failure(QueryError::response("err"));
    assert!(m.data().is_none());
    assert_eq!(m.error().unwrap().message(), "err");
    assert_eq!(m.retry_count(), 1);
}
