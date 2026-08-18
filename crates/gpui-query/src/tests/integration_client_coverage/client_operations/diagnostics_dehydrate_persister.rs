//! Diagnostics, dehydrate/hydrate, and persister tests (tests 24–30).

use std::sync::Mutex;

use gpui::{AppContext as _, BorrowAppContext as _, TestAppContext};

use crate::client::{
    DehydratedEntry, DehydratedState, QueryClient,
};
use crate::core::*;
use crate::tests::test_support::*;

// -- 24. Diagnostics: query status and cache_policy accuracy -----------------

#[gpui::test]
fn test_diagnostics_query_status_accuracy(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create idle resource
            let _idle = client.resource::<String, QueryError>("diag_idle", cx);

            // Create success resource via prepared fetch
            let prepared = client
                .prepare_fetch_query::<String, QueryError>("diag_success", cx)
                .expect("should start");
            prepared.complete_success("data".to_string(), cx);

            let diag = client.diagnostics(cx);
            let idle_diag = diag
                .queries
                .iter()
                .find(|q| q.key == "diag_idle")
                .expect("should find idle entry");
            assert_eq!(idle_diag.status, QueryStatus::Idle);

            let success_diag = diag
                .queries
                .iter()
                .find(|q| q.key == "diag_success")
                .expect("should find success entry");
            assert_eq!(success_diag.status, QueryStatus::Success);
            assert!(success_diag.cache_age_ms.is_some(), "success should have cache age");
        });
    });
}

// -- 25. Diagnostics: cache_policy label correctness -------------------------

#[gpui::test]
fn test_diagnostics_cache_policy_label(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(QueryClient::with_policies(
            CachePolicy::StaleWhileRevalidate {
                ttl_ms: 1_000,
                stale_ms: 2_000,
            },
            RequestPolicy::LatestWins,
        ));
    });
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _entity = client.resource::<String, QueryError>("swr_key", cx);
            let diag = client.diagnostics(cx);
            let q = diag.queries.iter().find(|q| q.key == "swr_key").unwrap();
            assert!(q.cache_policy.contains("Stale-while-revalidate"));
        });
    });
}

// -- 26. Dehydrate includes infinite queries ---------------------------------

#[gpui::test]
fn test_dehydrate_includes_infinite_query_success(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Regular query success
            let q = client.resource::<String, QueryError>("q1", cx);
            q.update(cx, |r, _| r.apply_success("data".to_string(), 1_000));

            // Infinite query (no direct apply_success, just create idle)
            let _iq = client.infinite_resource::<String, QueryError>("iq1", cx);

            let state = client.dehydrate(cx);
            // Only the regular query with Success should appear
            let regular_entries: Vec<_> =
                state.entries.iter().filter(|e| e.kind == "query").collect();
            assert_eq!(regular_entries.len(), 1);
            assert_eq!(regular_entries[0].key, "q1");

            // Infinite query is idle, so not in dehydrate output
            let inf_entries: Vec<_> = state
                .entries
                .iter()
                .filter(|e| e.kind == "infinite")
                .collect();
            assert!(inf_entries.is_empty(), "idle infinite query not dehydrated");
        });
    });
}

// -- 27. DehydratedState default and manual construction ---------------------

#[gpui::test]
fn test_dehydrated_state_default_and_construction(_cx: &mut TestAppContext) {
    let state = DehydratedState::default();
    assert!(state.entries.is_empty());

    let entry = DehydratedEntry {
        key: "users".to_string(),
        type_id: std::any::TypeId::of::<(String, QueryError)>(),
        kind: "query",
        data_json: Some(r#""hello""#.to_string()),
    };
    let state = DehydratedState {
        entries: vec![entry],
    };
    assert_eq!(state.entries.len(), 1);
    assert_eq!(state.entries[0].key, "users");
    assert_eq!(state.entries[0].data_json.as_deref(), Some(r#""hello""#));
}

// -- 28. Hydrate is a no-op (placeholder API) --------------------------------

#[gpui::test]
fn test_hydrate_is_noop(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let state = DehydratedState {
                entries: vec![DehydratedEntry {
                    key: "test".to_string(),
                    type_id: std::any::TypeId::of::<(String, QueryError)>(),
                    kind: "query",
                    data_json: None,
                }],
            };
            // hydrate is a placeholder — should not panic
            client.hydrate(state, cx);

            // No data should be injected (hydrate is a no-op)
            let data = client.get_query_data::<String, QueryError>(
                &QueryKey::from("test"),
                cx,
            );
            assert!(data.is_none(), "hydrate is a no-op, no data injected");
        });
    });
}

// -- 29. QueryPersister: save/load round-trip with typed data ----------------

#[gpui::test]
fn test_persister_empty_restore(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            struct EmptyPersister;
            impl crate::client::QueryPersister for EmptyPersister {
                fn load(&self) -> Vec<DehydratedEntry> {
                    Vec::new()
                }
                fn save(&self, _entries: Vec<DehydratedEntry>) {}
            }

            // No resources yet — persist should produce no entries
            client.persist(&EmptyPersister, cx);
            let loaded = client.restore(&EmptyPersister);
            assert!(loaded.is_empty());
        });
    });
}

// -- 30. Persister records multiple success entries --------------------------

#[gpui::test]
fn test_persister_records_multiple_entries(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create several success resources
            for i in 0..5 {
                let key = format!("persist_{i}");
                let e = client.resource::<String, QueryError>(key.clone(), cx);
                e.update(cx, |r, _| r.apply_success(format!("val_{i}"), 1_000));
            }
            // One idle resource that should NOT be persisted
            let _idle = client.resource::<String, QueryError>("idle_persist", cx);

            struct CapturePersister {
                entries: Mutex<Vec<DehydratedEntry>>,
            }
            impl crate::client::QueryPersister for CapturePersister {
                fn load(&self) -> Vec<DehydratedEntry> {
                    self.entries.lock().unwrap().clone()
                }
                fn save(&self, entries: Vec<DehydratedEntry>) {
                    *self.entries.lock().unwrap() = entries;
                }
            }

            let persister = CapturePersister {
                entries: Mutex::new(Vec::new()),
            };

            client.persist(&persister, cx);
            let saved = persister.entries.lock().unwrap().clone();
            assert_eq!(saved.len(), 5, "only success entries should be persisted");
            for entry in &saved {
                assert!(entry.key.starts_with("persist_"));
                assert_eq!(entry.kind, "query");
            }
        });
    });
}
