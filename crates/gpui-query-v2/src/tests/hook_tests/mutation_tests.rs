//! Tests for `use_mutation`, `mutate`, and `mutate_with_callbacks`.

use std::sync::{Arc, Mutex};

use gpui::{AppContext as _, Entity, TestAppContext};

use crate::core::{MutationResource, MutationStatus, QueryError, RetryPolicy};
use crate::hook::*;
use crate::tests::test_support::*;

// ── use_mutation ───────────────────────────────────────────────────────────

#[gpui::test]
fn test_use_mutation_creates_idle_entity(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        entity: Entity<MutationResource<String, String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);
        let resource = entity.read(cx);
        assert_eq!(resource.status(), MutationStatus::Idle);
        assert!(resource.data().is_none());
        H { entity }
    });

    cx.update(|cx| {
        assert_eq!(
            harness.read(cx).entity.read(cx).status(),
            MutationStatus::Idle
        );
    });
}

// ── mutate ─────────────────────────────────────────────────────────────────

#[gpui::test]
fn test_mutate_triggers_execution_and_completes(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);
        mutate(
            &entity,
            "input-vars".to_string(),
            |vars| async move { Ok::<_, QueryError>(format!("result-{}", vars)) },
            cx,
        );
        assert!(entity.read(cx).is_loading(), "should be Loading immediately");
        H { mutation: entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).mutation.read(cx);
        assert!(resource.is_success());
        assert_eq!(resource.data(), Some(&"result-input-vars".to_string()));
    });
}

#[gpui::test]
fn test_mutate_failure_stores_error(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>(
            MutationOptions::default(),
            cx,
        );
        mutate(
            &entity,
            "bad-input".to_string(),
            |_vars| async { Err::<String, _>(QueryError::response("mutation failed")) },
            cx,
        );
        H { mutation: entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).mutation.read(cx);
        assert!(resource.is_failure());
        assert!(resource.data().is_none());
        let err = resource.error().expect("should have error");
        assert!(err.to_string().contains("mutation failed"));
    });
}

// ── mutate_with_callbacks ──────────────────────────────────────────────────

#[gpui::test]
fn test_mutate_with_callbacks_success(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let success_called = Arc::new(Mutex::new(false));
    let settled_called = Arc::new(Mutex::new(false));
    let success_clone = success_called.clone();
    let settled_clone = settled_called.clone();

    #[allow(dead_code)]
    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let _harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);
        mutate_with_callbacks(
            &entity,
            "vars".to_string(),
            |v| async move { Ok::<_, QueryError>(format!("ok-{}", v)) },
            MutationCallbacks::new()
                .on_success(move |data| {
                    assert_eq!(data, "ok-vars");
                    *success_clone.lock().unwrap() = true;
                })
                .on_settled(move |opt_data, opt_err| {
                    assert!(opt_data.is_some());
                    assert!(opt_err.is_none());
                    *settled_clone.lock().unwrap() = true;
                }),
            cx,
        );
        H { mutation: entity }
    });

    cx.run_until_parked();

    assert!(*success_called.lock().unwrap(), "on_success should fire");
    assert!(*settled_called.lock().unwrap(), "on_settled should fire");
}

#[gpui::test]
fn test_mutate_with_callbacks_failure(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let error_called = Arc::new(Mutex::new(false));
    let settled_called = Arc::new(Mutex::new(false));
    let error_clone = error_called.clone();
    let settled_clone = settled_called.clone();

    #[allow(dead_code)]
    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let _harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>(
            MutationOptions {
                retry_policy: RetryPolicy::no_retries(),
                gc_time_ms: 300_000,
            },
            cx,
        );
        mutate_with_callbacks(
            &entity,
            "fail-input".to_string(),
            |_| async { Err::<String, _>(QueryError::response("cb-error")) },
            MutationCallbacks::<String, QueryError>::new()
                .on_error(move |err: &QueryError| {
                    assert!(err.to_string().contains("cb-error"));
                    *error_clone.lock().unwrap() = true;
                })
                .on_settled(move |opt_data, opt_err| {
                    assert!(opt_data.is_none());
                    assert!(opt_err.is_some());
                    *settled_clone.lock().unwrap() = true;
                }),
            cx,
        );
        H { mutation: entity }
    });

    cx.run_until_parked();

    assert!(*error_called.lock().unwrap(), "on_error should fire");
    assert!(*settled_called.lock().unwrap(), "on_settled should fire on failure");
}

// ── Concurrent mutate guard ────────────────────────────────────────────────

#[gpui::test]
fn test_mutate_rejects_concurrent_calls(cx: &mut TestAppContext) {
    setup_query_client(cx);

    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);

        // Start the first mutation.
        mutate(
            &entity,
            "first".to_string(),
            |_vars| async move { Ok::<_, QueryError>("first-result".to_string()) },
            cx,
        );
        assert!(entity.read(cx).is_loading());

        // Attempt a second mutate while the first is still loading.
        // The second call should be rejected (no-op) per audit fix #8.
        mutate(
            &entity,
            "second".to_string(),
            |_vars| async move { Ok::<_, QueryError>("second-result".to_string()) },
            cx,
        );

        H { mutation: entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).mutation.read(cx);
        assert!(resource.is_success());
        assert_eq!(resource.variables(), Some(&"first".to_string()));
        assert_eq!(resource.data(), Some(&"first-result".to_string()));
    });
}

// ── Mutation registered with QueryClient ────────────────────────────────────

#[gpui::test]
fn test_use_mutation_registers_with_client(cx: &mut TestAppContext) {
    setup_query_client(cx);

    #[allow(dead_code)]
    struct H {
        entity: Entity<MutationResource<String, String, QueryError>>,
    }

    let _harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);
        let mutations = use_mutation_state::<String, String, QueryError, _>(cx);
        assert_eq!(mutations.len(), 1, "one mutation should be registered");
        assert_eq!(mutations[0].entity_id(), entity.entity_id());
        H { entity }
    });
}

// ── use_mutation: double mutate while loading ───────────────────────────────

#[gpui::test]
fn test_mutate_double_while_loading_second_rejected(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let mutator_call_count = Arc::new(Mutex::new(0u32));
    let mc1 = mutator_call_count.clone();
    let mc2 = mutator_call_count.clone();

    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);

        // First mutate.
        mutate(
            &entity,
            "first".to_string(),
            move |v| {
                let mc1 = mc1.clone();
                async move {
                    *mc1.lock().unwrap() += 1;
                    Ok::<_, QueryError>(format!("result-{}", v))
                }
            },
            cx,
        );

        // Second mutate while still loading — should be rejected.
        mutate(
            &entity,
            "second".to_string(),
            move |v| {
                let mc2 = mc2.clone();
                async move {
                    *mc2.lock().unwrap() += 1;
                    Ok::<_, QueryError>(format!("result-{}", v))
                }
            },
            cx,
        );

        H { mutation: entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).mutation.read(cx);
        assert!(resource.is_success());
        assert_eq!(resource.data(), Some(&"result-first".to_string()));
    });
    assert_eq!(
        *mutator_call_count.lock().unwrap(),
        1,
        "second mutate should have been rejected, only one mutator call"
    );
}

// ── use_mutation: mutation with retry ───────────────────────────────────────

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

// ── use_mutation: mutation reset ────────────────────────────────────────────

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

// ── mutate_with_callbacks: all callbacks fire on success ────────────────────

#[gpui::test]
fn test_mutate_callbacks_all_fire_on_success(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let success_val = Arc::new(Mutex::new(String::new()));
    let error_val = Arc::new(Mutex::new(String::new()));
    let settled_data = Arc::new(Mutex::new(None::<String>));
    let settled_err = Arc::new(Mutex::new(None::<String>));

    let sv = success_val.clone();
    let ev = error_val.clone();
    let sd = settled_data.clone();
    let se = settled_err.clone();

    #[allow(dead_code)]
    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let _harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);
        mutate_with_callbacks(
            &entity,
            "all-cb".to_string(),
            |v| async move { Ok::<_, QueryError>(format!("yes-{}", v)) },
            MutationCallbacks::<String, QueryError>::new()
                .on_success(move |data: &String| {
                    *sv.lock().unwrap() = data.clone();
                })
                .on_error(move |err: &QueryError| {
                    *ev.lock().unwrap() = err.to_string();
                })
                .on_settled(move |opt_data: Option<&String>, opt_err: Option<&QueryError>| {
                    *sd.lock().unwrap() = opt_data.map(|d| d.to_string());
                    *se.lock().unwrap() = opt_err.map(|e| e.to_string());
                }),
            cx,
        );
        H { mutation: entity }
    });

    cx.run_until_parked();

    assert_eq!(*success_val.lock().unwrap(), "yes-all-cb", "on_success should fire with data");
    assert!(
        error_val.lock().unwrap().is_empty(),
        "on_error should NOT fire on success"
    );
    assert_eq!(
        *settled_data.lock().unwrap(),
        Some("yes-all-cb".to_string()),
        "on_settled should receive data on success"
    );
    assert!(
        settled_err.lock().unwrap().is_none(),
        "on_settled should NOT receive error on success"
    );
}

// ── mutate_with_callbacks: all callbacks fire on failure ────────────────────

#[gpui::test]
fn test_mutate_callbacks_all_fire_on_failure(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let success_fired = Arc::new(Mutex::new(false));
    let error_msg = Arc::new(Mutex::new(String::new()));
    let settled_data = Arc::new(Mutex::new(None::<String>));
    let settled_err = Arc::new(Mutex::new(None::<String>));

    let sf = success_fired.clone();
    let em = error_msg.clone();
    let sd = settled_data.clone();
    let se = settled_err.clone();

    #[allow(dead_code)]
    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let _harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>(
            MutationOptions {
                retry_policy: RetryPolicy::no_retries(),
                gc_time_ms: 300_000,
            },
            cx,
        );
        mutate_with_callbacks(
            &entity,
            "fail-cb".to_string(),
            |_| async { Err::<String, _>(QueryError::response("total-failure")) },
            MutationCallbacks::<String, QueryError>::new()
                .on_success(move |_data: &String| {
                    *sf.lock().unwrap() = true;
                })
                .on_error(move |err: &QueryError| {
                    *em.lock().unwrap() = err.to_string();
                })
                .on_settled(move |opt_data: Option<&String>, opt_err: Option<&QueryError>| {
                    *sd.lock().unwrap() = opt_data.map(|d| d.to_string());
                    *se.lock().unwrap() = opt_err.map(|e| e.to_string());
                }),
            cx,
        );
        H { mutation: entity }
    });

    cx.run_until_parked();

    assert!(
        !*success_fired.lock().unwrap(),
        "on_success should NOT fire on failure"
    );
    assert!(
        error_msg.lock().unwrap().contains("total-failure"),
        "on_error should fire with error message"
    );
    assert!(
        settled_data.lock().unwrap().is_none(),
        "on_settled should NOT receive data on failure"
    );
    assert!(
        settled_err.lock().unwrap().is_some(),
        "on_settled should receive error on failure"
    );
}

// ── mutate_with_callbacks: on_settled always fires ──────────────────────────

#[gpui::test]
fn test_mutate_callbacks_settled_always_fires_on_success(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let settled_called = Arc::new(Mutex::new(false));
    let sc = settled_called.clone();

    #[allow(dead_code)]
    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let _harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);
        // Only set on_settled, not on_success or on_error.
        mutate_with_callbacks(
            &entity,
            "settled-only".to_string(),
            |v| async move { Ok::<_, QueryError>(format!("s-{}", v)) },
            MutationCallbacks::<String, QueryError>::new().on_settled(move |opt_data, opt_err| {
                assert!(opt_data.is_some(), "settled should have data");
                assert!(opt_err.is_none(), "settled should not have error");
                *sc.lock().unwrap() = true;
            }),
            cx,
        );
        H { mutation: entity }
    });

    cx.run_until_parked();

    assert!(
        *settled_called.lock().unwrap(),
        "on_settled must fire on success even without on_success callback"
    );
}

#[gpui::test]
fn test_mutate_callbacks_settled_always_fires_on_failure(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let settled_called = Arc::new(Mutex::new(false));
    let sc = settled_called.clone();

    #[allow(dead_code)]
    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let _harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>(
            MutationOptions {
                retry_policy: RetryPolicy::no_retries(),
                gc_time_ms: 300_000,
            },
            cx,
        );
        mutate_with_callbacks(
            &entity,
            "settled-fail".to_string(),
            |_| async { Err::<String, _>(QueryError::response("fail")) },
            MutationCallbacks::<String, QueryError>::new().on_settled(move |opt_data, opt_err| {
                assert!(opt_data.is_none(), "settled should not have data on failure");
                assert!(opt_err.is_some(), "settled should have error on failure");
                *sc.lock().unwrap() = true;
            }),
            cx,
        );
        H { mutation: entity }
    });

    cx.run_until_parked();

    assert!(
        *settled_called.lock().unwrap(),
        "on_settled must fire on failure even without on_error callback"
    );
}

// ── use_mutation: mutation with custom retry policy via options ──────────────

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

// ── mutate_with_callbacks: concurrent call rejected ─────────────────────────

#[gpui::test]
fn test_mutate_with_callbacks_rejects_concurrent(cx: &mut TestAppContext) {
    setup_query_client(cx);

    let settled_count = Arc::new(Mutex::new(0u32));
    let sc = settled_count.clone();

    // Gate: the first mutation blocks until the test releases it after issuing
    // the second concurrent mutate_with_callbacks call. Uses AtomicBool +
    // executor.timer() instead of thread::sleep to avoid blocking the executor.
    use std::sync::atomic::{AtomicBool, Ordering};
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
