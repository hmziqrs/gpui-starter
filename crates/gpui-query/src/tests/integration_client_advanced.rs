//! Integration tests for data retention, optimistic updates, mutations, prefetch, and diagnostics.

use gpui::TestAppContext;

use crate::client::QueryClient;
use crate::core::*;
use crate::integration_client_fixtures::*;

// ── Data retention integration tests ──────────────────────────────────────

#[gpui::test]
fn resource_display_data_lifecycle(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from("user:1");
        let entity = client.resource::<User, QueryError>(key.clone(), cx);

        // 1. Before any fetch: no data, no placeholder
        assert_eq!(entity.read(cx).display_data(), None);

        // 2. Set placeholder data, then start loading
        entity.update(cx, |r, _| {
            r.set_placeholder_data(Some(User {
                id: 0,
                name: "Loading...".into(),
            }));
        });
        assert_eq!(
            entity.read(cx).display_data().unwrap().name,
            "Loading...",
            "placeholder should be used as display_data"
        );

        // 3. Start a request
        let sequencer = &mut RequestSequencer::new();
        entity.update(cx, |r, _| {
            let _ = r.begin_request(sequencer, 1_000, QueryFetchMode::Normal);
        });
        assert!(entity.read(cx).is_loading());
        assert_eq!(
            entity.read(cx).display_data().unwrap().name,
            "Loading...",
            "placeholder still visible while loading"
        );

        // 4. Complete with real data
        let request_id = entity.read(cx).active_request_id().unwrap();
        entity.update(cx, |r, _| {
            r.complete_current_success(
                request_id,
                User {
                    id: 1,
                    name: "Alice".into(),
                },
                1_200,
            )
        });
        assert_eq!(entity.read(cx).status(), QueryStatus::Success);
        assert_eq!(entity.read(cx).display_data().unwrap().name, "Alice");
        assert_eq!(
            entity.read(cx).placeholder_data().unwrap().name,
            "Loading...",
            "placeholder still stored but not returned by display_data"
        );
    });
}

#[gpui::test]
fn resource_rollback_after_optimistic_update(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from("user:1");
        let entity = client.resource::<User, QueryError>(key.clone(), cx);

        // 1. Populate with real data
        entity.update(cx, |r, _| {
            r.apply_success(
                User {
                    id: 1,
                    name: "Alice".into(),
                },
                1_000,
            );
        });
        assert_eq!(entity.read(cx).data().unwrap().name, "Alice");

        // 2. Optimistic update: set new data directly
        entity.update(cx, |r, _| {
            r.apply_success(
                User {
                    id: 1,
                    name: "Alice (updated)".into(),
                },
                1_100,
            );
        });
        assert_eq!(entity.read(cx).data().unwrap().name, "Alice (updated)");
        assert_eq!(
            entity.read(cx).previous_data().unwrap().name,
            "Alice",
            "previous_data holds the pre-optimistic value"
        );

        // 3. Simulate failure: rollback to previous
        let rolled_back = entity.update(cx, |r, _| r.rollback_to_previous());
        assert!(rolled_back);
        assert_eq!(entity.read(cx).data().unwrap().name, "Alice");
        assert_eq!(entity.read(cx).status(), QueryStatus::Success);
        assert_eq!(entity.read(cx).previous_data(), None);
    });
}

// ── Optimistic update client-level tests ─────────────────────────────────────

#[gpui::test]
fn client_set_query_data_sets_data(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from("user:1");
        let entity = client.resource::<User, QueryError>(key.clone(), cx);

        // Populate with real data
        entity.update(cx, |r, _| {
            r.apply_success(
                User {
                    id: 1,
                    name: "Alice".into(),
                },
                1_000,
            );
        });
        assert_eq!(entity.read(cx).data().unwrap().name, "Alice");

        // Optimistic update via client
        let set = client.set_query_data::<User, QueryError>(
            &key,
            User {
                id: 1,
                name: "Alice (optimistic)".into(),
            },
            cx,
        );
        assert!(
            set,
            "set_query_data should return true for existing resource"
        );
        assert_eq!(entity.read(cx).data().unwrap().name, "Alice (optimistic)");
        assert_eq!(
            entity.read(cx).previous_data().unwrap().name,
            "Alice",
            "previous_data should hold the pre-optimistic value"
        );
    });
}

#[gpui::test]
fn client_rollback_query_data_restores(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from("user:1");
        let entity = client.resource::<User, QueryError>(key.clone(), cx);

        // Populate and then optimistic update
        entity.update(cx, |r, _| {
            r.apply_success(
                User {
                    id: 1,
                    name: "Alice".into(),
                },
                1_000,
            );
        });
        client.set_query_data::<User, QueryError>(
            &key,
            User {
                id: 1,
                name: "Alice (optimistic)".into(),
            },
            cx,
        );
        assert_eq!(entity.read(cx).data().unwrap().name, "Alice (optimistic)");

        // Rollback via client
        let rolled_back = client.rollback_query_data::<User, QueryError>(&key, cx);
        assert!(
            rolled_back,
            "rollback should return true when previous data exists"
        );
        assert_eq!(entity.read(cx).data().unwrap().name, "Alice");
        assert_eq!(entity.read(cx).status(), QueryStatus::Success);
    });
}

#[gpui::test]
fn client_set_query_data_returns_false_for_missing_resource(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from("ghost");
        // No resource created for this key

        let set = client.set_query_data::<User, QueryError>(
            &key,
            User {
                id: 0,
                name: "Nobody".into(),
            },
            cx,
        );
        assert!(
            !set,
            "set_query_data should return false for nonexistent resource"
        );
    });
}

#[gpui::test]
fn client_rollback_query_data_returns_false_when_no_previous(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from("user:1");
        let entity = client.resource::<User, QueryError>(key.clone(), cx);

        // Set data directly (no previous_data)
        entity.update(cx, |r, _| {
            r.apply_success(
                User {
                    id: 1,
                    name: "Alice".into(),
                },
                1_000,
            );
        });
        // previous_data is None after first apply_success

        let rolled_back = client.rollback_query_data::<User, QueryError>(&key, cx);
        assert!(
            !rolled_back,
            "rollback should return false when no previous data"
        );
    });
}

#[gpui::test]
fn optimistic_update_full_lifecycle(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from(["users", "42"]);
        let entity = client.resource::<User, QueryError>(key.clone(), cx);

        // 1. Populate with real data
        entity.update(cx, |r, _| {
            r.apply_success(
                User {
                    id: 42,
                    name: "Carol".into(),
                },
                1_000,
            );
        });
        assert_eq!(entity.read(cx).data().unwrap().name, "Carol");

        // 2. Optimistic update before mutation
        client.set_query_data::<User, QueryError>(
            &key,
            User {
                id: 42,
                name: "Carol (saving...)".into(),
            },
            cx,
        );
        assert_eq!(entity.read(cx).data().unwrap().name, "Carol (saving...)");
        assert_eq!(entity.read(cx).previous_data().unwrap().name, "Carol");

        // 3. Start the mutation request
        let sequencer = &mut RequestSequencer::new();
        entity.update(cx, |r, _| {
            let _ = r.begin_request(sequencer, 1_100, QueryFetchMode::Normal);
        });
        assert!(entity.read(cx).is_loading());

        // 4. Mutation succeeds with real data from server
        let request_id = entity.read(cx).active_request_id().unwrap();
        entity.update(cx, |r, _| {
            r.complete_current_success(
                request_id,
                User {
                    id: 42,
                    name: "Carol (saved)".into(),
                },
                1_200,
            )
        });

        assert_eq!(entity.read(cx).status(), QueryStatus::Success);
        assert_eq!(entity.read(cx).data().unwrap().name, "Carol (saved)");
        assert_eq!(
            entity.read(cx).previous_data().unwrap().name,
            "Carol (saving...)",
            "previous_data should be the optimistic value"
        );
    });
}

#[gpui::test]
fn optimistic_update_rollback_on_failure(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from(["users", "42"]);
        let entity = client.resource::<User, QueryError>(key.clone(), cx);

        // 1. Populate with real data
        entity.update(cx, |r, _| {
            r.apply_success(
                User {
                    id: 42,
                    name: "Carol".into(),
                },
                1_000,
            );
        });

        // 2. Optimistic update
        client.set_query_data::<User, QueryError>(
            &key,
            User {
                id: 42,
                name: "Carol (saving...)".into(),
            },
            cx,
        );
        assert_eq!(entity.read(cx).data().unwrap().name, "Carol (saving...)");

        // 3. Start mutation request
        let sequencer = &mut RequestSequencer::new();
        entity.update(cx, |r, _| {
            let _ = r.begin_request(sequencer, 1_100, QueryFetchMode::Normal);
        });
        let request_id = entity.read(cx).active_request_id().unwrap();

        // 4. Mutation fails
        entity.update(cx, |r, _| {
            r.complete_current_failure(request_id, QueryError::cancelled("network error"))
        });

        assert_eq!(entity.read(cx).status(), QueryStatus::Failure);
        assert_eq!(
            entity.read(cx).data().unwrap().name,
            "Carol (saving...)",
            "failure preserves optimistic data"
        );
        assert_eq!(
            entity.read(cx).previous_data().unwrap().name,
            "Carol",
            "previous_data still holds the original"
        );

        // 5. Rollback to original
        let rolled_back = client.rollback_query_data::<User, QueryError>(&key, cx);
        assert!(rolled_back);
        assert_eq!(entity.read(cx).data().unwrap().name, "Carol");
        assert_eq!(entity.read(cx).status(), QueryStatus::Success);
    });
}

// ── Client mutation tests ──────────────────────────────────────────────────

#[gpui::test]
fn test_client_mutation_resource(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from("update-user");
        let entity = client.mutation_resource::<String, User, QueryError>(&key, cx);

        // Verify it starts idle
        assert!(entity.read(cx).is_idle());

        // Begin a mutation
        entity.update(cx, |m, _| {
            m.begin("new-name".to_string(), 0);
        });
        assert!(entity.read(cx).is_loading());
        assert_eq!(
            entity.read(cx).variables(),
            Some(&"new-name".to_string())
        );

        // Complete with success
        entity.update(cx, |m, _| {
            m.complete_success(User {
                id: 1,
                name: "Alice (updated)".into(),
            });
        });
        assert!(entity.read(cx).is_success());
        assert_eq!(entity.read(cx).data().unwrap().name, "Alice (updated)");
    });
}

#[gpui::test]
fn test_client_mutation_count(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        assert_eq!(client.mutation_count(), 0);

        let _m1 = client.mutation_resource::<String, User, QueryError>(
            &QueryKey::from("mutation:1"),
            cx,
        );
        assert_eq!(client.mutation_count(), 1);

        let _m2 = client.mutation_resource::<String, User, QueryError>(
            &QueryKey::from("mutation:2"),
            cx,
        );
        assert_eq!(client.mutation_count(), 2);

        // Same key returns same entity (deduplication)
        let _m3 = client.mutation_resource::<String, User, QueryError>(
            &QueryKey::from("mutation:1"),
            cx,
        );
        assert_eq!(client.mutation_count(), 2, "duplicate key should not create new resource");
    });
}

#[gpui::test]
fn test_client_mutation_retry_lifecycle(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client =
            QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins)
                .with_retry_policy(RetryPolicy::new(2));

        let key = QueryKey::from("flaky-mutation");
        let entity = client.mutation_resource::<String, User, QueryError>(&key, cx);

        // Begin and fail first attempt
        entity.update(cx, |m, _| {
            m.begin("data".to_string(), 0);
            m.complete_failure(QueryError::response("timeout"));
        });
        assert!(entity.read(cx).is_failure());
        assert_eq!(entity.read(cx).retry_count(), 1);

        // Retry
        entity.update(cx, |m, _| {
            assert!(m.retry());
        });
        assert!(entity.read(cx).is_loading());

        // Fail again
        entity.update(cx, |m, _| {
            m.complete_failure(QueryError::response("timeout again"));
        });
        assert_eq!(entity.read(cx).retry_count(), 2);
        assert!(!entity.read(cx).should_retry());
    });
}

// ── Client prefetch_query tests ─────────────────────────────────────────────

#[gpui::test]
fn test_client_prefetch_query(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(
            CachePolicy::Ttl { ttl_ms: 5_000 },
            RequestPolicy::LatestWins,
        );

        let key = QueryKey::from(["users", "42"]);
        assert_eq!(client.total_count(), 0);

        // Prefetch creates the resource and starts a request
        let started = client.prefetch_query::<User, QueryError>(
            &key,
            CachePolicy::Ttl { ttl_ms: 5_000 },
            RequestPolicy::LatestWins,
            1_000,
            cx,
        );
        assert!(started, "prefetch should start a new request");
        assert_eq!(client.total_count(), 1);

        // Complete the request
        let entity = client.resource::<User, QueryError>(key.clone(), cx);
        let request_id = entity.read(cx).active_request_id().unwrap();
        entity.update(cx, |r, _| {
            r.complete_current_success(request_id, default_user(), 1_200)
        });

        // Second prefetch at t=2_000: cache is fresh -> returns false
        let started = client.prefetch_query::<User, QueryError>(
            &key,
            CachePolicy::Ttl { ttl_ms: 5_000 },
            RequestPolicy::LatestWins,
            2_000,
            cx,
        );
        assert!(!started, "prefetch should not start when cache is fresh");
    });
}

#[gpui::test]
fn test_client_prefetch_query_stale_cache(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(
            CachePolicy::Ttl { ttl_ms: 1_000 },
            RequestPolicy::LatestWins,
        );

        let key = QueryKey::from("user:1");
        let entity = client.resource::<User, QueryError>(key.clone(), cx);
        entity.update(cx, |r, _| r.apply_success(default_user(), 1_000));

        // Prefetch at t=2_000: cache is stale (age=1000, ttl=1000, age > ttl is false but age == ttl is fresh)
        let started = client.prefetch_query::<User, QueryError>(
            &key,
            CachePolicy::Ttl { ttl_ms: 1_000 },
            RequestPolicy::LatestWins,
            2_000,
            cx,
        );
        // age=1000, ttl=1000 -> age <= ttl -> fresh -> cache hit -> no request
        assert!(
            !started,
            "prefetch at exact TTL boundary should be a cache hit"
        );

        // Prefetch at t=2_001: cache is stale (age=1001 > ttl=1000)
        let started = client.prefetch_query::<User, QueryError>(
            &key,
            CachePolicy::Ttl { ttl_ms: 1_000 },
            RequestPolicy::LatestWins,
            2_001,
            cx,
        );
        assert!(
            started,
            "prefetch should start a new request when cache is stale"
        );
    });
}

// ── Client diagnostics tests ────────────────────────────────────────────────

#[gpui::test]
fn test_client_diagnostics(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        );

        let u1 = client.resource::<User, QueryError>(QueryKey::from(["users", "1"]), cx);
        u1.update(cx, |r, _| r.apply_success(default_user(), 1_000));

        let _p1 = client.resource::<Post, QueryError>(QueryKey::from(["posts", "1"]), cx);

        let _m1 = client.mutation_resource::<String, User, QueryError>(
            &QueryKey::from("mutation:1"),
            cx,
        );

        let diag = client.diagnostics(cx);
        assert_eq!(diag.total_resources, 2);
        assert_eq!(diag.bucket_count, 2);
        assert_eq!(diag.mutation_count, 1);
        assert_eq!(diag.queries.len(), 2, "should have diagnostics for both queries");

        // Find the user query diagnostic
        let user_diag = diag
            .queries
            .iter()
            .find(|q| q.key.contains("users"))
            .expect("should find user query");
        assert!(user_diag.has_data);
        assert!(!user_diag.has_error);
        assert_eq!(user_diag.status, "Success");
    });
}

#[gpui::test]
fn test_client_query_diagnostics_by_type(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        );

        let _u = client.resource::<User, QueryError>(QueryKey::from("user:1"), cx);
        let _p = client.resource::<Post, QueryError>(QueryKey::from("post:1"), cx);

        // Query diagnostics for User type only
        let user_diags = client.query_diagnostics::<User, QueryError>(cx);
        assert_eq!(user_diags.len(), 1);
        assert!(user_diags[0].key.contains("user"));

        // Query diagnostics for Post type only
        let post_diags = client.query_diagnostics::<Post, QueryError>(cx);
        assert_eq!(post_diags.len(), 1);
        assert!(post_diags[0].key.contains("post"));

        // Query diagnostics for a type that has no bucket
        let empty_diags = client.query_diagnostics::<String, QueryError>(cx);
        assert!(empty_diags.is_empty());
    });
}
