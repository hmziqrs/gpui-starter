//! Integration tests for optimistic updates via the client.

use gpui::TestAppContext;

use crate::client::QueryClient;
use crate::core::*;
use crate::integration_client_fixtures::*;

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
