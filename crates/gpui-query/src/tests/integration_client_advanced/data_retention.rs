//! Integration tests for data retention and rollback at the resource level.

use gpui::TestAppContext;

use crate::client::QueryClient;
use crate::core::*;
use crate::integration_client_fixtures::*;

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
