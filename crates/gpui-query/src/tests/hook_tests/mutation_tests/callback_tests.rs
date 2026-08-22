//! Tests for `mutate_with_callbacks` success, failure, and settled callback behavior.

use std::sync::{Arc, Mutex};

use gpui::{AppContext as _, Entity, TestAppContext};

use crate::core::{MutationResource, QueryError, RetryPolicy};
use crate::hook::*;
use crate::tests::test_support::*;

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
