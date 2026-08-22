//! Cancellation, signal propagation, and cancelled_count tests.

use crate::core::*;

// -- Cancellation cancels signal and sets Failure --

#[test]
fn cancel_during_loading_sets_failure() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    m.begin("vars".to_string());

    let signal = m.signal().unwrap().clone();
    assert!(!signal.is_cancelled());

    m.cancel(QueryError::cancelled("user aborted"));

    assert!(m.is_failure());
    assert_eq!(m.error().unwrap().kind(), QueryErrorKind::Cancelled);
    assert_eq!(m.error().unwrap().message(), "user aborted");
    assert!(signal.is_cancelled(), "external signal clone must see cancellation");
    assert!(m.signal().is_none(), "signal cleared after cancel");
    assert_eq!(m.cancelled_count(), 1);
}

// -- Cancel is a no-op on Idle, Success, Failure --

#[test]
fn cancel_on_idle_is_noop() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    m.cancel(QueryError::cancelled("x"));
    assert!(m.is_idle());
    assert_eq!(m.cancelled_count(), 0);
    assert!(m.error().is_none());
}

#[test]
fn cancel_on_success_is_noop() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    m.begin("vars".to_string());
    m.complete_success(10);
    m.cancel(QueryError::cancelled("x"));
    assert!(m.is_success());
    assert_eq!(m.data(), Some(&10));
    assert_eq!(m.cancelled_count(), 0);
}

#[test]
fn cancel_on_failure_is_noop() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());
    m.begin("vars".to_string());
    m.complete_failure(QueryError::response("fail"));
    let retry_count_before = m.retry_count();
    m.cancel(QueryError::cancelled("x"));
    assert!(m.is_failure());
    assert_eq!(m.retry_count(), retry_count_before);
    assert_eq!(m.cancelled_count(), 0);
}

// -- cancelled_count increments across multiple cancellations --

#[test]
fn cancelled_count_increments_across_mutations() {
    let mut m: MutationResource<String, i32> = MutationResource::new(RetryPolicy::no_retries());

    m.begin("first".to_string());
    m.cancel(QueryError::cancelled("abort 1"));
    assert_eq!(m.cancelled_count(), 1);

    m.begin("second".to_string());
    m.cancel(QueryError::cancelled("abort 2"));
    assert_eq!(m.cancelled_count(), 2);

    m.begin("third".to_string());
    m.cancel(QueryError::cancelled("abort 3"));
    assert_eq!(m.cancelled_count(), 3);
}
