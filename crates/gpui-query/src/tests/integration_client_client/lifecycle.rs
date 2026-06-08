use gpui::TestAppContext;

use crate::client::QueryClient;
use crate::core::*;
use crate::integration_client_fixtures::*;

#[gpui::test]
fn full_lifecycle_idle_to_loading_to_success_to_gc(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(
            CachePolicy::Ttl { ttl_ms: 1_000 },
            RequestPolicy::LatestWins,
        )
        .with_gc_time(5_000);

        // 1. Create resource
        let key = QueryKey::from(["users", "42"]);
        let entity = client.resource::<User, QueryError>(key.clone(), cx);
        assert_eq!(entity.read(cx).status(), QueryStatus::Idle);
        assert_eq!(client.total_count(), 1);

        // 2. Start request directly on the entity
        let sequencer = &mut RequestSequencer::new();
        entity.update(cx, |r, _| {
            let result = r.begin_request(sequencer, 1_000, QueryFetchMode::Normal);
            assert!(matches!(result, QueryBeginResult::Started { .. }));
        });
        assert!(entity.read(cx).is_loading());

        // 3. Complete with success at t=1_200
        let request_id = entity.read(cx).active_request_id().unwrap();
        let success = entity.update(cx, |r, _| {
            r.complete_current_success(
                request_id,
                User {
                    id: 42,
                    name: "Carol".into(),
                },
                1_200,
            )
        });
        assert!(success);
        assert_eq!(entity.read(cx).status(), QueryStatus::Success);
        assert_eq!(entity.read(cx).data().unwrap().name, "Carol");
        assert!(entity.read(cx).is_cache_fresh(1_500));

        // 4. GC before gc_time (age = 2_800 - 1_200 = 1_600 < 5_000) → kept
        client.gc(cx, 2_800);
        assert_eq!(client.total_count(), 1);

        // 5. GC after gc_time (age = 10_000 - 1_200 = 8_800 > 5_000) → collected
        client.gc(cx, 10_000);
        assert_eq!(client.total_count(), 0);
    });
}

#[gpui::test]
fn invalidated_resource_survives_gc_because_timestamp_is_cleared(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(
            CachePolicy::Ttl { ttl_ms: 1_000 },
            RequestPolicy::LatestWins,
        )
        .with_gc_time(1_000);

        let key = QueryKey::from("user:1");
        let entity = client.resource::<User, QueryError>(key.clone(), cx);
        entity.update(cx, |r, _| r.apply_success(default_user(), 100));

        // Invalidate clears last_updated_at → GC can't determine age → resource kept
        client.invalidate_queries(&QueryKeyFilter::All, cx);
        assert!(
            entity.read(cx).data().is_some(),
            "data survives invalidation"
        );
        assert!(
            entity.read(cx).last_updated_at_ms().is_none(),
            "timestamp cleared"
        );

        client.gc(cx, 100_000);
        assert_eq!(client.total_count(), 1, "invalidated resource survives GC");
    });
}
