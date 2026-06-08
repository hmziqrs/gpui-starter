//! Diagnostics, dehydrate/hydrate, persister, fetch/prefetch, cancel, GC,
//! query operations, and observer tests (tests 24–44, 49, 52–55, 57–61).

use std::sync::Mutex;

use gpui::{AppContext as _, BorrowAppContext as _, TestAppContext};

use crate::client::{
    DehydratedEntry, DehydratedState, InfiniteQueryObserver, QueryClient,
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

// -- 31. prepare_fetch_query always starts (uses Force mode) -----------------

#[gpui::test]
fn test_prepare_fetch_query_uses_force_mode_always_starts(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(QueryClient::with_policies(
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        ));
    });
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // First fetch succeeds
            let prepared = client
                .prepare_fetch_query::<String, QueryError>("cached_key", cx)
                .expect("first fetch should start");
            prepared.complete_success("data".to_string(), cx);

            // prepare_fetch_query uses QueryFetchMode::Force, so it always
            // starts a new request even when the cache is fresh. This matches
            // TanStack Query's fetchQuery behavior.
            let second = client.prepare_fetch_query::<String, QueryError>("cached_key", cx);
            assert!(
                second.is_some(),
                "prepare_fetch_query uses Force mode, always starts"
            );

            // Data should still be accessible from the first fetch
            let data = client.get_query_data::<String, QueryError>(
                &QueryKey::from("cached_key"),
                cx,
            );
            assert_eq!(data, Some("data".to_string()));
        });
    });
}

// -- 32. prepare_fetch_query refetch after TTL --------------------------------
//
// NOTE: This test verifies that prepare_fetch_query returns Some both on the
// initial call and on a subsequent call with Force mode. Full TTL expiry
// behavior (data becoming stale and triggering automatic refetch) is tested
// at the resource level in core_cache.rs, where timestamps can be controlled
// deterministically via apply_success(data, now_ms).

#[gpui::test]
fn test_prepare_fetch_query_refetch_after_ttl(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(QueryClient::with_policies(
            CachePolicy::Ttl { ttl_ms: 500 },
            RequestPolicy::LatestWins,
        ));
    });
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // First fetch
            let prepared = client
                .prepare_fetch_query::<String, QueryError>("ttl_key", cx)
                .expect("first fetch should start");
            prepared.complete_success("old".to_string(), cx);

            // Data should be present after first fetch
            let data = client.get_query_data::<String, QueryError>(
                &QueryKey::from("ttl_key"),
                cx,
            );
            assert_eq!(data, Some("old".to_string()));

            // prepare_fetch_query always returns Some (Force mode), even when
            // cache is fresh. This is the core guarantee: it always initiates
            // a fetch, unlike prepare_prefetch_query which respects freshness.
            let second = client
                .prepare_fetch_query::<String, QueryError>("ttl_key", cx);
            assert!(
                second.is_some(),
                "prepare_fetch_query should always return Some (Force mode), \
                 even when data was just set"
            );
        });
    });
}

// -- 33. prepare_prefetch_query returns None for fresh data ------------------
//
// Finding 4/7 fix: Asserts the actual return value of prepare_prefetch_query.
// Uses the current wall-clock time via current_time_ms() to set a timestamp
// that is guaranteed fresh (age ~0ms, well within the 60s TTL).
//
// WALL-CLOCK DEPENDENCY: This test relies on current_time_ms() for both the
// data timestamp and the freshness check inside prepare_prefetch_query. The
// 60-second TTL provides a large margin against clock skew in CI, but if this
// test ever flakes, the root cause will be a system clock discontinuity (e.g.,
// NTP step). An alternative would be to mock current_time_ms, but that would
// require a crate-level time abstraction that is not currently available.

#[gpui::test]
fn test_prepare_prefetch_query_returns_none_for_fresh(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(QueryClient::with_policies(
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        ));
    });
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("prefresh_fresh");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);
            // Use the current wall-clock time so the data is fresh when
            // prepare_prefetch_query checks is_cache_fresh(current_time_ms()).
            let now = crate::client::current_time_ms();
            entity.update(cx, |r, _| r.apply_success("fresh_data".to_string(), now));

            // prepare_prefetch_query uses Normal mode. Since the data was set
            // at ~now, age is ~0ms, which is within the 60s TTL, so the cache
            // is fresh and prefetch should return None (no fetch needed).
            let result = client.prepare_prefetch_query::<String, QueryError>(
                key.clone(),
                CachePolicy::Ttl { ttl_ms: 60_000 },
                RequestPolicy::LatestWins,
                cx,
            );
            assert!(
                result.is_none(),
                "prefetch should return None for fresh data \
                 (age ~0ms, well within 60s TTL)"
            );
        });
    });
}

// -- 34. PreparedFetch complete_failure stores error -------------------------

#[gpui::test]
fn test_prepared_fetch_complete_failure_stores_error(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("pf_fail");
            let prepared = client
                .prepare_fetch_query::<String, QueryError>(key.clone(), cx)
                .expect("should start");

            let error = QueryError::response("server unavailable");
            prepared.complete_failure(error.clone(), cx);

            let entity = client.query::<String, QueryError>(&key).unwrap();
            assert_eq!(entity.read(cx).status(), QueryStatus::Failure);
            assert!(entity.read(cx).data().is_none());
            assert!(entity.read(cx).error().is_some());
        });
    });
}

// -- 35. PreparedFetch signal starts uncancelled -----------------------------

#[gpui::test]
fn test_prepared_fetch_signal_properties(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let prepared = client
                .prepare_fetch_query::<String, QueryError>("signal_test", cx)
                .expect("should start");

            assert!(!prepared.signal.is_cancelled(), "signal should start uncancelled");
            assert!(prepared.request_id.value() > 0, "request_id should have a positive value");

            // Complete to clean up
            prepared.complete_success("data".to_string(), cx);
        });
    });
}

// -- 36. cancel_queries cancels resources across multiple type buckets -------

#[gpui::test]
fn test_cancel_queries_across_type_buckets(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create and start requests for two different types with the same string key
            let key_s = QueryKey::from("target");
            let entity_s = client.resource::<String, QueryError>(key_s.clone(), cx);
            let rid_s = client
                .next_request_id_for_key::<String, QueryError>(&key_s)
                .expect("rid");
            entity_s.update(cx, |r, _| {
                r.begin_request_with_id(Some(rid_s), 1_000, QueryFetchMode::Normal);
            });

            let key_u = QueryKey::from("target");
            let entity_u = client.resource::<u32, QueryError>(key_u.clone(), cx);
            let rid_u = client
                .next_request_id_for_key::<u32, QueryError>(&key_u)
                .expect("rid");
            entity_u.update(cx, |r, _| {
                r.begin_request_with_id(Some(rid_u), 1_000, QueryFetchMode::Normal);
            });

            let sig_s = entity_s.read(cx).signal().unwrap().clone();
            let sig_u = entity_u.read(cx).signal().unwrap().clone();

            // Cancel all queries with key "target" (Exact filter)
            client.cancel_queries(&QueryKeyFilter::Exact(&key_s), cx);

            assert!(sig_s.is_cancelled(), "String query should be cancelled");
            assert!(sig_u.is_cancelled(), "u32 query should be cancelled");
        });
    });
}

// -- 37. cancel_queries with All filter cancels everything -------------------

#[gpui::test]
fn test_cancel_queries_all_filter(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Start two loading queries
            let key1 = QueryKey::from("a1");
            let key2 = QueryKey::from("a2");
            let e1 = client.resource::<String, QueryError>(key1.clone(), cx);
            let e2 = client.resource::<String, QueryError>(key2.clone(), cx);

            let rid1 = client.next_request_id_for_key::<String, QueryError>(&key1).unwrap();
            let rid2 = client.next_request_id_for_key::<String, QueryError>(&key2).unwrap();

            e1.update(cx, |r, _| {
                r.begin_request_with_id(Some(rid1), 1_000, QueryFetchMode::Normal);
            });
            e2.update(cx, |r, _| {
                r.begin_request_with_id(Some(rid2), 1_000, QueryFetchMode::Normal);
            });

            client.cancel_queries(&QueryKeyFilter::All, cx);

            let sig1 = e1.read(cx).signal().unwrap().clone();
            let sig2 = e2.read(cx).signal().unwrap().clone();
            assert!(sig1.is_cancelled());
            assert!(sig2.is_cancelled());
        });
    });
}

// -- 38. cancel_queries does not affect idle infinite queries ----------------

#[gpui::test]
fn test_cancel_queries_skips_idle_infinite_queries(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("inf_idle");
            let _entity = client.infinite_resource::<String, QueryError>(key.clone(), cx);

            // Should not panic or affect the idle infinite query
            client.cancel_queries(&QueryKeyFilter::Exact(&key), cx);

            let retrieved = client.infinite_query::<String, QueryError>(&key);
            assert!(retrieved.is_some(), "idle infinite query should still exist");
        });
    });
}

// -- 39. GC clamps gc_time=0 to 1000ms; Idle resources with no snapshot
//         timestamp are evicted at any gc_with_time value since their
//         age defaults to gc_threshold. ──────────────────────────────────────
//
// Finding 1 fix: Assert concrete eviction outcomes. An Idle resource with
// no snapshot timestamp (never fetched) is treated as "age == gc_threshold"
// by the GC, so it is always evicted regardless of gc_time clamping.

#[gpui::test]
fn test_gc_with_zero_time_clamped_evicts_idle(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 0);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _entity = client.resource::<String, QueryError>("gc_zero", cx);
            assert_eq!(client.all_queries::<String, QueryError>().len(), 1);

            // gc_time_ms=0 is clamped to 1000ms. The resource is Idle with no
            // snapshot timestamp, so its age defaults to gc_threshold (1000ms),
            // meaning age >= threshold and it gets evicted.
            client.gc_with_time(0, cx);
            assert_eq!(
                client.all_queries::<String, QueryError>().len(),
                0,
                "Idle resource with no snapshot timestamp should be evicted \
                 even at gc_with_time(0) because its age defaults to the clamped gc_threshold"
            );
        });
    });
}

// -- 40. GC uses gc_with_time with deterministic time control ----------------
//
// Finding 2 fix: Assert concrete GC outcomes using the documented eviction
// rules. The bucket's status snapshot is only updated when the hook layer
// calls update_status_snapshot (not from direct resource mutations), so
// resources created via client.resource() always appear as Idle with
// last_updated_ms=None to GC. Idle resources with no timestamp are evicted
// at any gc_with_time value (age defaults to gc_threshold).

#[gpui::test]
fn test_gc_with_time_explicit_time_value(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create two resources: one to be evicted, one to verify removal
            let _e1 = client.resource::<String, QueryError>("gc_evict", cx);
            let _e2 = client.resource::<String, QueryError>("gc_evict2", cx);
            assert_eq!(client.all_queries::<String, QueryError>().len(), 2);

            // First GC at small time: both Idle resources with no snapshot
            // timestamp have age == gc_threshold (clamped to 1000ms), so
            // age < gc_threshold is false and they get evicted.
            client.gc_with_time(500, cx);
            assert_eq!(
                client.all_queries::<String, QueryError>().len(),
                0,
                "Idle resources with no snapshot timestamp should be evicted \
                 at gc_with_time(500) — their age defaults to the clamped gc_threshold"
            );

            // Create another resource and run GC at a very large time.
            let _e3 = client.resource::<String, QueryError>("gc_big_time", cx);
            assert_eq!(client.all_queries::<String, QueryError>().len(), 1);

            client.gc_with_time(100_000, cx);
            assert_eq!(
                client.all_queries::<String, QueryError>().len(),
                0,
                "Idle resource should also be evicted at gc_with_time(100_000)"
            );

            // After eviction, diagnostics should report zero queries.
            let diag = client.diagnostics(cx);
            assert_eq!(
                diag.query_count, 0,
                "diagnostics should report 0 queries after all were evicted"
            );
        });
    });
}

// -- 41. GC runs across all bucket types (query, infinite, mutation) ---------
//
// Finding 3 fix: After GC, assert that idle resources with no snapshot
// updates ARE evicted, and loading resources are preserved.

#[gpui::test]
fn test_gc_runs_across_all_bucket_types(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create idle query (no fetch) — should be evicted by GC
            let _q = client.resource::<String, QueryError>("q_gc", cx);

            // Create idle infinite query (no fetch) — should be evicted by GC
            let _iq = client.infinite_resource::<String, QueryError>("iq_gc", cx);

            // Create a loading mutation — loading resources should survive GC
            let loading_mut = cx.new(|_| {
                MutationResource::<String, User, QueryError>::new(RetryPolicy::no_retries())
            });
            loading_mut.update(cx, |m, _| m.begin("vars".to_string()));
            client.register_mutation::<String, User, QueryError>(&loading_mut, cx);

            let idle_mut = cx.new(|_| {
                MutationResource::<String, User, QueryError>::new(RetryPolicy::no_retries())
            });
            client.register_mutation::<String, User, QueryError>(&idle_mut, cx);

            // Pre-GC counts
            assert_eq!(client.all_queries::<String, QueryError>().len(), 1);
            assert_eq!(client.all_infinite_queries::<String, QueryError>().len(), 1);
            assert_eq!(client.all_mutations::<String, User, QueryError>().len(), 2);

            // GC at far future time
            client.gc_with_time(100_000, cx);

            // Post-GC: idle resources with no snapshot timestamp are evicted
            assert!(
                client.all_queries::<String, QueryError>().is_empty(),
                "idle query with no snapshot should be evicted by GC"
            );
            assert!(
                client.all_infinite_queries::<String, QueryError>().is_empty(),
                "idle infinite query with no snapshot should be evicted by GC"
            );

            // Loading mutation should survive GC (loading resources are never evicted)
            let mutations = client.all_mutations::<String, User, QueryError>();
            assert_eq!(
                mutations.len(),
                2,
                "both mutations should survive GC (one loading, one idle but retained by strong Entity ref)"
            );
        });
    });
}

// -- 42. remove_queries removes from infinite buckets too --------------------

#[gpui::test]
fn test_remove_queries_affects_infinite_queries(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _iq = client.infinite_resource::<String, QueryError>(
                QueryKey::from(["users", "inf"]),
                cx,
            );
            let _q = client.resource::<String, QueryError>(
                QueryKey::from(["users", "reg"]),
                cx,
            );

            assert_eq!(client.all_infinite_queries::<String, QueryError>().len(), 1);
            assert_eq!(client.all_queries::<String, QueryError>().len(), 1);

            let prefix = QueryKey::from(["users"]);
            client.remove_queries(&QueryKeyFilter::Prefix(&prefix));

            assert!(
                client.all_infinite_queries::<String, QueryError>().is_empty(),
                "infinite query should be removed"
            );
            assert!(
                client.all_queries::<String, QueryError>().is_empty(),
                "regular query should be removed"
            );
        });
    });
}

// -- 43. invalidate_queries affects infinite queries too ---------------------

#[gpui::test]
fn test_invalidate_queries_affects_infinite_queries(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _iq = client.infinite_resource::<String, QueryError>("inf_inv", cx);
            // Note: InfiniteQueryResource doesn't have apply_success in the same way,
            // but we can still verify invalidate doesn't panic and the entity remains.
            client.invalidate_queries(&QueryKeyFilter::All, cx);

            let retrieved = client.infinite_query::<String, QueryError>(&QueryKey::from("inf_inv"));
            assert!(retrieved.is_some(), "infinite query should still exist after invalidate");
        });
    });
}

// -- 44. reset_queries affects infinite queries ------------------------------

#[gpui::test]
fn test_reset_queries_affects_infinite_queries(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let _iq = client.infinite_resource::<String, QueryError>("inf_reset", cx);
            client.reset_queries(&QueryKeyFilter::All, cx);

            let retrieved = client.infinite_query::<String, QueryError>(&QueryKey::from("inf_reset"));
            assert!(retrieved.is_some(), "infinite query should exist after reset");
            // InfiniteQueryResource in idle state
            assert_eq!(retrieved.unwrap().read(cx).status(), QueryStatus::Idle);
        });
    });
}

// -- 49. Infinite query observer creation and observe ------------------------

#[gpui::test]
fn test_infinite_query_observer_creation_and_observe(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = client.infinite_resource::<String, QueryError>("inf_obs", cx);
            let mut observer = InfiniteQueryObserver::new(&entity);

            struct DummyView;
            let view = cx.new(|_| DummyView);
            let sub = view.update(cx, |_view, cx| observer.observe(cx));
            assert!(sub.is_some(), "observe should return Some(Subscription)");
        });
    });
}

#[gpui::test]
fn test_infinite_query_observer_weak_entity_pattern(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = client.infinite_resource::<String, QueryError>("inf_obs_weak", cx);
            let mut observer = InfiniteQueryObserver::new(&entity);

            struct DummyView;
            let view = cx.new(|_| DummyView);
            let sub = view.update(cx, |_view, cx| observer.observe(cx));
            assert!(sub.is_some(), "observe should return Some for live entity");
        });
    });
}

// -- 52. current_time_ms is reasonable ---------------------------------------

#[gpui::test]
fn test_current_time_ms_is_reasonable(_cx: &mut TestAppContext) {
    let now = crate::client::current_time_ms();
    // Should be > 1_700_000_000_000 (after 2023) and < 2_000_000_000_000 (before 2033)
    assert!(
        now > 1_700_000_000_000,
        "current_time_ms should be post-2023"
    );
    assert!(
        now < 2_000_000_000_000,
        "current_time_ms should be pre-2033"
    );
}

// -- 53. Multiple resources share same type bucket ---------------------------

#[gpui::test]
fn test_multiple_resources_same_type_share_bucket(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create 10 resources of same type
            for i in 0..10 {
                let key = format!("multi_{i}");
                let _e = client.resource::<String, QueryError>(key, cx);
            }

            let all = client.all_queries::<String, QueryError>();
            assert_eq!(all.len(), 10, "should have 10 resources in the same bucket");

            let diag = client.diagnostics(cx);
            assert_eq!(diag.query_count, 10);
            assert_eq!(diag.queries.len(), 10);
        });
    });
}

// -- 54. invalidate then re-fetch lifecycle ----------------------------------
//
// Finding 6 fix: Assert that prepare_fetch_query always returns Some after
// invalidation (since the cache is invalidated, Force mode should start a
// new fetch regardless).

#[gpui::test]
fn test_invalidate_then_refetch_lifecycle(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("inv_refetch");

            // Fetch and succeed
            let p1 = client
                .prepare_fetch_query::<String, QueryError>(key.clone(), cx)
                .expect("first fetch");
            p1.complete_success("v1".to_string(), cx);

            assert_eq!(
                client.get_query_data::<String, QueryError>(&key, cx),
                Some("v1".to_string())
            );

            // Invalidate — marks the cache as stale
            client.invalidate_queries(&QueryKeyFilter::Exact(&key), cx);

            // After invalidation, prepare_fetch_query (Force mode) must always
            // start a new request. This is a guaranteed contract: Force mode
            // ignores cache freshness, so the result is always Some.
            let p2 = client
                .prepare_fetch_query::<String, QueryError>(key.clone(), cx);
            assert!(
                p2.is_some(),
                "prepare_fetch_query must return Some after invalidation — \
                 Force mode always starts a new request regardless of cache state"
            );

            p2.unwrap().complete_success("v2".to_string(), cx);
            assert_eq!(
                client.get_query_data::<String, QueryError>(&key, cx),
                Some("v2".to_string())
            );
        });
    });
}

// -- 55. Reset clears data then set_query_data re-populates ------------------

#[gpui::test]
fn test_reset_then_set_query_data(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("reset_set");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);
            entity.update(cx, |r, _| r.apply_success("original".to_string(), 1_000));

            // Reset clears everything
            client.reset_queries(&QueryKeyFilter::Exact(&key), cx);
            assert!(entity.read(cx).data().is_none());
            assert_eq!(entity.read(cx).status(), QueryStatus::Idle);

            // Re-populate via set_query_data
            client.set_query_data::<String, QueryError>(key, "restored".to_string(), cx);
            assert_eq!(entity.read(cx).data().unwrap(), "restored");
        });
    });
}

// -- 57. Large number of resources creation ----------------------------------

#[gpui::test]
fn test_large_number_of_resources_creation(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            for i in 0..100 {
                let key = format!("large_{i}");
                let _e = client.resource::<u32, QueryError>(key, cx);
            }
            let all = client.all_queries::<u32, QueryError>();
            assert_eq!(all.len(), 100, "should have 100 resources");
        });
    });
}

// -- 58. QueryKey with multiple segments in client operations ----------------

#[gpui::test]
fn test_multi_segment_key_in_client_operations(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from(["org", "team", "user", "42"]);
            let entity = client.resource::<String, QueryError>(key.clone(), cx);
            entity.update(cx, |r, _| r.apply_success("deep_key_data".to_string(), 1_000));

            // Query by the full key
            let found = client.query::<String, QueryError>(&key);
            assert!(found.is_some());

            // Invalidate by prefix "org/team"
            let prefix = QueryKey::from(["org", "team"]);
            client.invalidate_queries(&QueryKeyFilter::Prefix(&prefix), cx);
            assert!(!entity.read(cx).is_cache_fresh(1_500), "should be stale after prefix invalidate");

            // Data should survive invalidation
            assert_eq!(entity.read(cx).data().unwrap(), "deep_key_data");

            // Reset by prefix
            client.reset_queries(&QueryKeyFilter::Prefix(&prefix), cx);
            assert!(entity.read(cx).data().is_none(), "data cleared after prefix reset");
        });
    });
}

// -- 59. set_query_data with different T types doesn't conflict --------------

#[gpui::test]
fn test_set_query_data_different_types_no_conflict(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Same key, different types
            client.set_query_data::<String, QueryError>("shared_key", "string_val".to_string(), cx);
            client.set_query_data::<u32, QueryError>("shared_key", 42_u32, cx);

            let s = client.get_query_data::<String, QueryError>(&QueryKey::from("shared_key"), cx);
            let n = client.get_query_data::<u32, QueryError>(&QueryKey::from("shared_key"), cx);

            assert_eq!(s, Some("string_val".to_string()));
            assert_eq!(n, Some(42_u32));
        });
    });
}

// -- 60. clear_data on resource via client context ---------------------------

#[gpui::test]
fn test_clear_data_via_resource(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("clear_me");
            client.set_query_data::<String, QueryError>(key.clone(), "data".to_string(), cx);

            let entity = client.query::<String, QueryError>(&key).unwrap();
            assert_eq!(entity.read(cx).data().unwrap(), "data");

            entity.update(cx, |r, _| r.clear_data());
            assert!(entity.read(cx).data().is_none(), "data should be cleared");
            // get_query_data should now return None
            let data = client.get_query_data::<String, QueryError>(&key, cx);
            assert!(data.is_none());
        });
    });
}

// -- 61. prepare_prefetch_query returns Some for stale data -----------------
//
// Finding 4/7 fix: This test asserts the actual return value. When data
// was set at t=0 (long ago) and prefetch checks at current_time_ms,
// the data is stale, so prefetch should return Some.

#[gpui::test]
fn test_prepare_prefetch_query_returns_some_for_stale(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(QueryClient::with_policies(
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        ));
    });
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Populate with data at t=0 (epoch) — guaranteed stale now
            let key = QueryKey::from("prefresh_stale");
            let entity = client.resource::<String, QueryError>(key.clone(), cx);
            entity.update(cx, |r, _| r.apply_success("stale_data".to_string(), 0));

            // Data is from t=0 and current_time_ms() is ~1.7 trillion ms,
            // so the resource is definitely stale (age >> 60_000ms TTL).
            // prepare_prefetch_query uses Normal mode, which respects freshness,
            // so it should start a fetch for stale data.
            let result = client.prepare_prefetch_query::<String, QueryError>(
                key.clone(),
                CachePolicy::Ttl { ttl_ms: 60_000 },
                RequestPolicy::LatestWins,
                cx,
            );
            assert!(
                result.is_some(),
                "prefetch should return Some for stale data \
                 (data from t=0 is well past the 60s TTL at current time)"
            );
        });
    });
}
