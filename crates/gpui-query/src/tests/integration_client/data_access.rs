//! Tests for data access: signal cancellation, query removal, set/rollback
//! query data, dehydrate/hydrate, request ID sequences, PreparedFetch,
//! prefetch, and persistence.

use std::sync::Mutex;

use gpui::{AppContext as _, BorrowAppContext as _, TestAppContext};

use crate::client::QueryClient;
use crate::core::*;
use crate::tests::test_support::*;

// ── 9. Signal retrieval and cancellation ───────────────────────────────

#[gpui::test]
fn test_cancel_queries_cancels_loading_requests(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("cancel_target");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);

            // Start a request
            let request_id = client
                .next_request_id_for_key::<String, QueryError>(&key)
                .expect("should get request id");
            entity.update(cx, |r, _| {
                r.begin_request_with_id(
                    Some(request_id),
                    1_000,
                    QueryFetchMode::Normal,
                );
            });
            assert!(entity.read(cx).is_loading());

            // Grab the signal before cancelling
            let signal = entity
                .read(cx)
                .signal()
                .expect("signal should exist while loading")
                .clone();
            assert!(!signal.is_cancelled());

            // Cancel via client bulk operation
            client.cancel_queries(&QueryKeyFilter::Exact(&key), cx);

            assert!(
                signal.is_cancelled(),
                "signal should be cancelled after cancel_queries"
            );
        });
    });
}

#[gpui::test]
fn test_cancel_queries_skips_idle_resources(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("idle_key");
            let _entity = client.resource::<String, QueryError>(key.clone(), cx);

            // Resource is idle — cancel_queries should be a no-op
            client.cancel_queries(&QueryKeyFilter::Exact(&key), cx);

            let entity = client.query::<String, QueryError>(&key);
            assert!(entity.is_some(), "resource should still exist");
            assert_eq!(
                entity.unwrap().read(cx).status(),
                QueryStatus::Idle,
                "status should remain Idle"
            );
        });
    });
}

#[gpui::test]
fn test_remove_queries_removes_matching(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _u1 = client.resource::<String, QueryError>(
                QueryKey::from(["users", "1"]),
                cx,
            );
            let _u2 = client.resource::<String, QueryError>(
                QueryKey::from(["users", "2"]),
                cx,
            );
            let _p1 = client.resource::<String, QueryError>(
                QueryKey::from(["posts", "1"]),
                cx,
            );

            assert_eq!(client.all_queries::<String, QueryError>().len(), 3);

            let prefix = QueryKey::from(["users"]);
            client.remove_queries(&QueryKeyFilter::Prefix(&prefix));

            let remaining = client.all_queries::<String, QueryError>();
            assert_eq!(remaining.len(), 1, "only the post should remain");
            assert!(
                client
                    .query::<String, QueryError>(&QueryKey::from(["posts", "1"]))
                    .is_some(),
                "post should still exist"
            );
            assert!(
                client
                    .query::<String, QueryError>(&QueryKey::from(["users", "1"]))
                    .is_none(),
                "user:1 should be removed"
            );
        });
    });
}

// ── 10. set_query_data / rollback_query_data ───────────────────────────

#[gpui::test]
fn test_set_query_data_sets_data_on_existing_resource(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("user:1");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);

            // Populate with real data
            entity.update(cx, |r, _| {
                r.apply_success("Alice".to_string(), 1_000);
            });
            assert_eq!(entity.read(cx).data().unwrap(), "Alice");

            // Overwrite via set_query_data
            client.set_query_data::<String, QueryError>(
                "user:1",
                "Bob".to_string(),
                cx,
            );

            assert_eq!(entity.read(cx).data().unwrap(), "Bob");
            assert_eq!(
                entity.read(cx).previous_data().unwrap(),
                "Alice",
                "previous_data should hold the pre-set value"
            );
        });
    });
}

#[gpui::test]
fn test_set_query_data_creates_resource_if_missing(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // set_query_data on a key that does not exist yet
            client.set_query_data::<String, QueryError>(
                "new_key",
                "hello".to_string(),
                cx,
            );

            let data = client.get_query_data::<String, QueryError>(
                &QueryKey::from("new_key"),
                cx,
            );
            assert_eq!(data, Some("hello".to_string()));
        });
    });
}

#[gpui::test]
fn test_get_query_data_returns_none_for_missing_key(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let data = client.get_query_data::<String, QueryError>(
                &QueryKey::from("ghost"),
                cx,
            );
            assert!(data.is_none(), "should return None for nonexistent key");
        });
    });
}

#[gpui::test]
fn test_rollback_query_data_via_resource(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("user:1");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);

            // Populate then optimistic update
            entity.update(cx, |r, _| r.apply_success("Alice".to_string(), 1_000));
            client.set_query_data::<String, QueryError>(
                "user:1",
                "Alice (optimistic)".to_string(),
                cx,
            );
            assert_eq!(
                entity.read(cx).data().unwrap(),
                "Alice (optimistic)"
            );

            // Rollback via the resource directly
            let rolled_back = entity.update(cx, |r, _| r.rollback_to_previous());
            assert!(rolled_back);
            assert_eq!(entity.read(cx).data().unwrap(), "Alice");
            assert_eq!(entity.read(cx).status(), QueryStatus::Success);
        });
    });
}

// ── 14. Dehydrate / hydrate ────────────────────────────────────────────

#[gpui::test]
fn test_dehydrate_collects_success_entries(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Success resource — should appear in dehydrate
            let e1 = client.resource::<String, QueryError>(QueryKey::from("ok"), cx);
            e1.update(cx, |r, _| r.apply_success("data".to_string(), 1_000));

            // Idle resource — should NOT appear
            let _e2 =
                client.resource::<String, QueryError>(QueryKey::from("idle"), cx);

            let state = client.dehydrate(cx);
            assert_eq!(
                state.entries.len(),
                1,
                "only Success resources should be dehydrated"
            );
            assert_eq!(state.entries[0].key, "ok");
            assert_eq!(state.entries[0].kind, "query");
        });
    });
}

#[gpui::test]
fn test_dehydrate_skips_non_success_resources(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Failure resource
            let e1 = client.resource::<String, QueryError>("fail", cx);
            e1.update(cx, |r, _| {
                r.apply_failure(QueryError::response("err"), 1_000)
            });

            // Idle (no data)
            let _e2 = client.resource::<String, QueryError>("idle2", cx);

            let state = client.dehydrate(cx);
            assert!(
                state.entries.is_empty(),
                "non-Success resources should not be dehydrated"
            );
        });
    });
}

// ── 15. next_request_id_for_key monotonic sequence ─────────────────────

#[gpui::test]
fn test_next_request_id_is_monotonically_increasing(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("seq_test");
            let _entity = client.resource::<String, QueryError>(key.clone(), cx);

            let id1 = client.next_request_id_for_key::<String, QueryError>(&key);
            let id2 = client.next_request_id_for_key::<String, QueryError>(&key);
            let id3 = client.next_request_id_for_key::<String, QueryError>(&key);

            assert!(id1.is_some());
            assert!(id2.is_some());
            assert!(id3.is_some());

            let id1 = id1.unwrap();
            let id2 = id2.unwrap();
            let id3 = id3.unwrap();

            assert!(id1.value() < id2.value(), "sequence should be increasing");
            assert!(id2.value() < id3.value(), "sequence should be increasing");
            assert_eq!(id1.scope_id(), id2.scope_id(), "same scope for same key");
        });
    });
}

#[gpui::test]
fn test_next_request_id_returns_none_for_missing_key(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, _cx| {
            let id = client.next_request_id_for_key::<String, QueryError>(
                &QueryKey::from("ghost"),
            );
            assert!(id.is_none(), "should return None for nonexistent key");
        });
    });
}

// ── 16. PreparedFetch lifecycle ────────────────────────────────────────

#[gpui::test]
fn test_prepare_fetch_query_success_lifecycle(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("fetch_key");
            let prepared = client.prepare_fetch_query::<String, QueryError>(key.clone(), cx);

            assert!(prepared.is_some(), "should return Some for new resource");
            let prepared = prepared.unwrap();
            assert!(
                !prepared.signal.is_cancelled(),
                "signal should start uncancelled"
            );

            // Complete with success
            prepared.complete_success("fetched_data".to_string(), cx);

            // Verify data is stored
            let data = client.get_query_data::<String, QueryError>(&key, cx);
            assert_eq!(data, Some("fetched_data".to_string()));
        });
    });
}

#[gpui::test]
fn test_prepare_fetch_query_failure_lifecycle(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("fail_fetch");
            let prepared = client.prepare_fetch_query::<String, QueryError>(key.clone(), cx);
            assert!(prepared.is_some());

            let prepared = prepared.unwrap();
            prepared.complete_failure(QueryError::response("server error"), cx);

            let entity = client.query::<String, QueryError>(&key).unwrap();
            assert_eq!(entity.read(cx).status(), QueryStatus::Failure);
        });
    });
}

// ── 17. prepare_prefetch_query ─────────────────────────────────────────

#[gpui::test]
fn test_prepare_prefetch_query_starts_for_stale_resource(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("prefetch_key");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);

            // Populate with stale data (old timestamp)
            entity.update(cx, |r, _| r.apply_success("old_data".to_string(), 100));

            // Prefetch should start since data is stale at t=5000 with TTL=1000
            let prepared = client.prepare_prefetch_query::<String, QueryError>(
                key.clone(),
                CachePolicy::Ttl { ttl_ms: 1_000 },
                RequestPolicy::LatestWins,
                cx,
            );
            assert!(
                prepared.is_some(),
                "prefetch should start for stale resource"
            );

            let prepared = prepared.unwrap();
            prepared.complete_success("fresh_data".to_string(), cx);

            let data = client.get_query_data::<String, QueryError>(&key, cx);
            assert_eq!(data, Some("fresh_data".to_string()));
        });
    });
}

// ── 18. Persistence trait integration ──────────────────────────────────

#[gpui::test]
fn test_persist_and_restore_cycle(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create a success resource
            let entity = client.resource::<String, QueryError>("persist_me", cx);
            entity.update(cx, |r, _| r.apply_success("value".to_string(), 1_000));

            // Mock persister backed by in-memory storage
            struct MemPersister {
                entries: Mutex<Vec<crate::client::DehydratedEntry>>,
            }
            impl crate::client::QueryPersister for MemPersister {
                fn load(&self) -> Vec<crate::client::DehydratedEntry> {
                    self.entries.lock().unwrap().clone()
                }
                fn save(&self, entries: Vec<crate::client::DehydratedEntry>) {
                    *self.entries.lock().unwrap() = entries;
                }
            }

            let persister = MemPersister {
                entries: Mutex::new(Vec::new()),
            };

            // Persist
            client.persist(&persister, cx);

            // Restore
            let loaded = client.restore(&persister);
            assert_eq!(loaded.len(), 1, "should have one persisted entry");
            assert_eq!(loaded[0].key, "persist_me");
        });
    });
}
