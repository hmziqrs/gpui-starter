//! Tests for mutation retry behavior, reset, custom retry policy, and concurrent callback rejection.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use gpui::{AppContext as _, Entity, TestAppContext};

use crate::core::{MutationResource, MutationStatus, QueryError, RetryPolicy};
use crate::hook::*;
use crate::tests::test_support::*;

#[gpui::test]
fn test_mutation_retries_on_failure(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let call_count = Arc::new(Mutex::new(0u32));
    let cc = call_count.clone();

    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let retry_opts = MutationOptions {
            retry_policy: RetryPolicy::new(2).with_delay(0),
            gc_time_ms: 300_000,
        };
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>(retry_opts, cx);
        mutate(
            &entity,
            "retry-input".to_string(),
            move |_vars| {
                let cc = cc.clone();
                async move {
                    let mut n = cc.lock().unwrap();
                    *n += 1;
                    if *n < 3 {
                        Err::<String, _>(QueryError::response("transient"))
                    } else {
                        Ok::<_, QueryError>("recovered".to_string())
                    }
                }
            },
            cx,
        );
        H { mutation: entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).mutation.read(cx);
        assert!(
            resource.is_success(),
            "mutation should succeed after retries, got {:?}",
            resource.status()
        );
        assert_eq!(resource.data(), Some(&"recovered".to_string()));
    });
    assert_eq!(
        *call_count.lock().unwrap(),
        3,
        "should have taken 3 attempts (2 failures + 1 success)"
    );
}

#[gpui::test]
fn test_mutation_reset_clears_state(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);
        mutate(
            &entity,
            "reset-input".to_string(),
            |_v| async { Ok::<_, QueryError>("reset-result".to_string()) },
            cx,
        );
        H { mutation: entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).mutation.read(cx);
        assert!(resource.is_success());
        assert_eq!(resource.data(), Some(&"reset-result".to_string()));
    });

    // Reset the mutation in a separate update to avoid borrow conflict.
    let mutation = cx.update(|cx| harness.read(cx).mutation.clone());
    cx.update(|cx| {
        mutation.update(cx, |m, _| {
            m.reset();
        });
    });

    cx.update(|cx| {
        let resource = harness.read(cx).mutation.read(cx);
        assert_eq!(resource.status(), MutationStatus::Idle);
        assert!(resource.data().is_none());
        assert!(resource.error().is_none());
        assert!(resource.variables().is_none());
    });
}

#[gpui::test]
fn test_mutation_custom_retry_policy(cx: &mut TestAppContext) {
    setup_query_client(cx);

    #[allow(dead_code)]
    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>(
            MutationOptions {
                retry_policy: RetryPolicy::new(5).with_delay(1),
                gc_time_ms: 300_000,
            },
            cx,
        );
        let policy = entity.read(cx).retry_policy();
        assert_eq!(policy.max_retries, 5);
        H { mutation: entity }
    });

    let _ = harness;
}

#[gpui::test]
fn test_mutate_with_callbacks_rejects_concurrent(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let settled_count = Arc::new(Mutex::new(0u32));
    let sc = settled_count.clone();

    // Gate: the first mutation blocks until the test releases it after issuing
    // the second concurrent mutate_with_callbacks call. Uses AtomicBool +
    // executor.timer() instead of thread::sleep to avoid blocking the executor.
    let gate = Arc::new(AtomicBool::new(false));
    let gate_clone = gate.clone();
    let executor = cx.background_executor.clone();

    #[allow(dead_code)]
    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let _harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);

        let executor = executor.clone();
        mutate_with_callbacks(
            &entity,
            "first".to_string(),
            move |_v| {
                let gate_clone = gate_clone.clone();
                let executor = executor.clone();
                async move {
                    // Wait for the gate using executor-aware yield instead of
                    // thread::sleep. This allows the second mutation call to be
                    // scheduled while we wait.
                    while !gate_clone.load(Ordering::Acquire) {
                        executor.timer(std::time::Duration::from_millis(1)).await;
                    }
                    Ok::<_, QueryError>("first-result".to_string())
                }
            },
            MutationCallbacks::<String, QueryError>::new().on_settled(move |_, _| {
                *sc.lock().unwrap() += 1;
            }),
            cx,
        );

        // Second concurrent call should be rejected.
        mutate_with_callbacks(
            &entity,
            "second".to_string(),
            |_v| async move { Ok::<_, QueryError>("second-result".to_string()) },
            MutationCallbacks::<String, QueryError>::new(),
            cx,
        );

        H { mutation: entity }
    });

    // Release the gate so the first mutation can complete.
    gate.store(true, Ordering::Release);

    cx.run_until_parked();

    assert_eq!(
        *settled_count.lock().unwrap(),
        1,
        "only the first mutation's callbacks should fire"
    );
}
