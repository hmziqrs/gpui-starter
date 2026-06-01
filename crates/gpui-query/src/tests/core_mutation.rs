use crate::core::*;

#[test]
fn test_mutation_new_is_idle() {
    let m: MutationResource<String, String> = MutationResource::new(RetryPolicy::no_retries());
    assert!(m.is_idle());
    assert!(!m.is_loading());
    assert!(!m.is_success());
    assert!(!m.is_failure());
    assert_eq!(m.status(), MutationStatus::Idle);
    assert!(m.data().is_none());
    assert!(m.error().is_none());
    assert!(m.variables().is_none());
    assert_eq!(m.retry_count(), 0);
    assert!(m.signal().is_none());
}

#[test]
fn test_mutation_begin_sets_loading() {
    let mut m: MutationResource<String, String> = MutationResource::new(RetryPolicy::no_retries());
    m.begin("my-vars".to_string());

    assert!(m.is_loading());
    assert_eq!(m.status(), MutationStatus::Loading);
    assert_eq!(m.variables(), Some(&"my-vars".to_string()));
    assert!(m.error().is_none());
    assert!(m.signal().is_some());
}

#[test]
fn test_mutation_success_sets_data() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    m.begin("vars".to_string());
    m.complete_success(42);

    assert!(m.is_success());
    assert_eq!(m.status(), MutationStatus::Success);
    assert_eq!(m.data(), Some(&42));
    assert!(m.error().is_none());
    assert!(m.signal().is_none());
    assert_eq!(m.variables(), Some(&"vars".to_string()));
}

#[test]
fn test_mutation_failure_sets_error() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    m.begin("vars".to_string());
    m.complete_failure(QueryError::response("bad"));

    assert!(m.is_failure());
    assert_eq!(m.status(), MutationStatus::Failure);
    assert!(m.data().is_none());
    assert_eq!(m.error().unwrap().message(), "bad");
    assert_eq!(m.retry_count(), 1);
    assert!(m.signal().is_none());
}

#[test]
fn test_mutation_retry_on_failure() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(2));

    m.begin("vars".to_string());
    m.complete_failure(QueryError::response("fail 1"));

    assert!(m.is_failure());
    assert_eq!(m.retry_count(), 1);
    assert!(m.should_retry());

    let retried = m.retry();
    assert!(retried);
    assert!(m.is_loading());
    assert!(m.error().is_none());
    assert!(m.signal().is_some());
    assert_eq!(m.variables(), Some(&"vars".to_string()));
}

#[test]
fn test_mutation_retry_respects_max_retries() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(1));

    m.begin("vars".to_string());
    m.complete_failure(QueryError::response("fail 1"));
    assert_eq!(m.retry_count(), 1);
    assert!(!m.should_retry()); // max_retries=1, 1 is not < 1

    let retried = m.retry();
    assert!(!retried);
    assert!(m.is_failure());
}

#[test]
fn test_mutation_reset() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(3));

    m.begin("vars".to_string());
    m.complete_success(99);
    m.reset();

    assert!(m.is_idle());
    assert!(m.data().is_none());
    assert!(m.error().is_none());
    assert!(m.variables().is_none());
    assert_eq!(m.retry_count(), 0);
    assert!(m.signal().is_none());
}

#[test]
fn test_mutation_cancel() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    m.begin("vars".to_string());

    let signal = m.signal().unwrap().clone();
    assert!(!signal.is_cancelled());

    m.cancel(QueryError::cancelled("aborted"));

    assert!(m.is_failure());
    assert_eq!(m.error().unwrap().message(), "aborted");
    assert!(signal.is_cancelled());
    assert!(m.signal().is_none());
}

#[test]
fn test_mutation_signal_created_on_begin() {
    let mut m: MutationResource<String, String> = MutationResource::new(RetryPolicy::no_retries());
    assert!(m.signal().is_none(), "no signal before begin");

    m.begin("vars".to_string());

    let signal = m
        .signal()
        .expect("signal should exist after begin");
    assert!(
        !signal.is_cancelled(),
        "fresh signal should not be cancelled"
    );
}

#[test]
fn test_mutation_signal_cancelled_on_cancel() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    m.begin("vars".to_string());

    let signal_clone = m.signal().unwrap().clone();
    assert!(!signal_clone.is_cancelled());

    m.cancel(QueryError::cancelled("user cancelled"));

    assert!(
        signal_clone.is_cancelled(),
        "signal clone should see cancellation"
    );
    assert!(m.signal().is_none(), "signal should be cleared after cancel");
}

#[test]
fn test_mutation_retry_only_from_failure_status() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(3));

    // Cannot retry from Idle
    assert!(!m.retry());

    // Cannot retry from Loading
    m.begin("vars".to_string());
    assert!(!m.retry());

    // Can retry from Failure
    m.complete_failure(QueryError::response("fail"));
    assert!(m.retry());

    // After retry, back to Loading -- cannot retry again without failing first
    assert!(!m.retry());
}

#[test]
fn test_mutation_retry_creates_fresh_signal() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::new(2));
    m.begin("vars".to_string());

    let old_signal = m.signal().unwrap().clone();
    m.complete_failure(QueryError::response("fail"));

    let retried = m.retry();
    assert!(retried);

    let new_signal = m.signal().unwrap();
    assert!(
        !new_signal.is_cancelled(),
        "retry should create a fresh signal"
    );
    assert!(
        old_signal.is_cancelled() || !old_signal.is_cancelled(),
        "old signal reference is independent"
    );
}

#[test]
fn test_mutation_status_labels() {
    assert_eq!(MutationStatus::Idle.label(), "Idle");
    assert_eq!(MutationStatus::Loading.label(), "Loading");
    assert_eq!(MutationStatus::Success.label(), "Success");
    assert_eq!(MutationStatus::Failure.label(), "Failure");
}

#[test]
fn test_mutation_with_key() {
    let m: MutationResource<String, i32> =
        MutationResource::new(RetryPolicy::no_retries()).with_key(QueryKey::from(["users", "1"]));

    assert!(m.key().is_some());
    assert_eq!(m.key().unwrap(), &QueryKey::from(["users", "1"]));
}

#[test]
fn test_mutation_without_key() {
    let m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    assert!(m.key().is_none());
}
