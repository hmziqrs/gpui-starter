use crate::core::*;
use crate::test_support::resource;

#[test]
fn test_no_retries_should_not_retry() {
    let policy = RetryPolicy::no_retries();
    assert_eq!(policy.max_retries, 0);
    assert!(!policy.should_retry(0));
    assert!(!policy.should_retry(1));
}

#[test]
fn test_default_policy_should_retry_up_to_max() {
    let policy = RetryPolicy::default();
    assert_eq!(policy.max_retries, 3);

    assert!(policy.should_retry(0));
    assert!(policy.should_retry(1));
    assert!(policy.should_retry(2));
    assert!(!policy.should_retry(3));
    assert!(!policy.should_retry(10));
}

#[test]
fn test_delay_for_attempt_fixed() {
    let policy = RetryPolicy::new(3).with_delay(500);
    assert_eq!(policy.delay_for_attempt(0), 500);
    assert_eq!(policy.delay_for_attempt(1), 500);
    assert_eq!(policy.delay_for_attempt(5), 500);
}

#[test]
fn test_delay_for_attempt_exponential_backoff() {
    let policy = RetryPolicy::new(5)
        .with_delay(1000)
        .with_exponential_backoff()
        .with_max_delay(30_000);

    assert_eq!(policy.delay_for_attempt(0), 1000); // 1000 * 2^0
    assert_eq!(policy.delay_for_attempt(1), 2000); // 1000 * 2^1
    assert_eq!(policy.delay_for_attempt(2), 4000); // 1000 * 2^2
    assert_eq!(policy.delay_for_attempt(3), 8000); // 1000 * 2^3
    assert_eq!(policy.delay_for_attempt(4), 16000); // 1000 * 2^4
}

#[test]
fn test_delay_for_attempt_capped_at_max() {
    let policy = RetryPolicy::new(10)
        .with_delay(500)
        .with_exponential_backoff()
        .with_max_delay(2000);

    assert_eq!(policy.delay_for_attempt(0), 500);
    assert_eq!(policy.delay_for_attempt(1), 1000);
    assert_eq!(policy.delay_for_attempt(2), 2000); // exactly at cap
    assert_eq!(policy.delay_for_attempt(3), 2000); // capped
    assert_eq!(policy.delay_for_attempt(10), 2000); // still capped
}

#[test]
fn test_builder_pattern() {
    let policy = RetryPolicy::new(2)
        .with_delay(200)
        .with_exponential_backoff()
        .with_max_delay(5000);

    assert_eq!(policy.max_retries, 2);
    assert_eq!(policy.retry_delay_ms, 200);
    assert!(policy.exponential_backoff);
    assert_eq!(policy.max_retry_delay_ms, 5000);
}

#[test]
fn test_retry_count_on_query_resource() {
    let mut resource = resource();
    assert_eq!(resource.retry_count(), 0);

    resource.increment_retry();
    assert_eq!(resource.retry_count(), 1);

    resource.increment_retry();
    assert_eq!(resource.retry_count(), 2);
}

#[test]
fn test_increment_retry() {
    let mut resource = resource();

    resource.increment_retry();
    resource.increment_retry();
    resource.increment_retry();

    assert_eq!(resource.retry_count(), 3);
}

#[test]
fn test_set_retry_policy() {
    let mut resource = resource();
    assert_eq!(resource.retry_policy().max_retries, 0); // no_retries by default

    let policy = RetryPolicy::new(5)
        .with_delay(500)
        .with_exponential_backoff()
        .with_max_delay(10_000);
    resource.set_retry_policy(policy.clone());

    assert_eq!(resource.retry_policy().max_retries, 5);
    assert_eq!(resource.retry_policy().retry_delay_ms, 500);
    assert!(resource.retry_policy().exponential_backoff);
    assert_eq!(resource.retry_policy().max_retry_delay_ms, 10_000);
}

#[test]
fn test_reset_retry_count() {
    let mut resource = resource();

    resource.increment_retry();
    resource.increment_retry();
    assert_eq!(resource.retry_count(), 2);

    resource.reset_retry_count();
    assert_eq!(resource.retry_count(), 0);
}

#[test]
fn test_reset_clears_retry_count() {
    let mut resource = resource();
    resource.increment_retry();
    resource.increment_retry();
    assert_eq!(resource.retry_count(), 2);

    resource.reset();
    assert_eq!(resource.retry_count(), 0);
}
