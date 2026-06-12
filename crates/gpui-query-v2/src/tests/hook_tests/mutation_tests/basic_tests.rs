//! Basic mutation tests: creation, mutate, failure, client registration, concurrent guard.

use std::sync::{Arc, Mutex};

use gpui::{AppContext as _, Entity, TestAppContext};

use crate::core::{MutationResource, MutationStatus, QueryError};
use crate::hook::*;
use crate::tests::test_support::*;

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
        assert!(
            entity.read(cx).is_loading(),
            "should be Loading immediately"
        );
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
        let (entity, _sub) =
            use_mutation::<String, String, QueryError, _>(MutationOptions::default(), cx);
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
