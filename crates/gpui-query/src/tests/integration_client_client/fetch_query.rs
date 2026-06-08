use gpui::TestAppContext;

use crate::client::{BucketDefaults, QueryBucket, QueryClient, QueryBucketTrait};
use crate::core::*;
use crate::integration_client_fixtures::*;

#[gpui::test]
fn client_fetch_query_creates_resource_and_starts_request(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from(["users", "42"]);
        assert_eq!(client.total_count(), 0, "client should start empty");

        let result = client.fetch_query::<User, QueryError>(
            key.clone(),
            CachePolicy::NoCache,
            RequestPolicy::LatestWins,
            1_000,
            cx,
        );

        let (entity, request_id) = result.expect("fetch_query should return Some for new resource");
        assert_eq!(client.total_count(), 1, "resource should be created");
        assert!(entity.read(cx).is_loading(), "resource should be loading");
        assert!(entity.read(cx).active_request_id().is_some());
        assert_eq!(request_id, entity.read(cx).active_request_id().unwrap());
    });
}

#[gpui::test]
fn client_fetch_query_returns_none_on_cache_hit(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(
            CachePolicy::Ttl { ttl_ms: 5_000 },
            RequestPolicy::LatestWins,
        );

        let key = QueryKey::from(["users", "1"]);

        // First fetch starts a request
        let (entity, request_id) = client
            .fetch_query::<User, QueryError>(
                key.clone(),
                CachePolicy::Ttl { ttl_ms: 5_000 },
                RequestPolicy::LatestWins,
                1_000,
                cx,
            )
            .expect("first fetch should start");

        // Complete the request with success
        entity.update(cx, |r, _| {
            r.complete_current_success(request_id, default_user(), 1_200)
        });

        // Second fetch at t=2_000: cache is fresh (age = 800 < 5000)
        let result = client.fetch_query::<User, QueryError>(
            key.clone(),
            CachePolicy::Ttl { ttl_ms: 5_000 },
            RequestPolicy::LatestWins,
            2_000,
            cx,
        );

        assert!(result.is_none(), "should return None on cache hit");
        assert_eq!(
            entity.read(cx).cache_hits(),
            1,
            "cache hit should be recorded"
        );
    });
}

#[gpui::test]
fn client_fetch_query_returns_none_on_ignore_while_loading(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::IgnoreWhileLoading);

        let key = QueryKey::from(["users", "5"]);

        // First fetch starts a request
        let _ = client
            .fetch_query::<User, QueryError>(
                key.clone(),
                CachePolicy::NoCache,
                RequestPolicy::IgnoreWhileLoading,
                1_000,
                cx,
            )
            .expect("first fetch should start");

        // Second fetch with IgnoreWhileLoading: request is already loading
        let result = client.fetch_query::<User, QueryError>(
            key.clone(),
            CachePolicy::NoCache,
            RequestPolicy::IgnoreWhileLoading,
            1_500,
            cx,
        );

        assert!(
            result.is_none(),
            "should return None when ignored while loading"
        );
    });
}

#[gpui::test]
fn client_force_fetch_query_bypasses_cache(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(
            CachePolicy::Ttl { ttl_ms: 5_000 },
            RequestPolicy::LatestWins,
        );

        let key = QueryKey::from(["users", "1"]);

        // First fetch: start and complete
        let (entity, request_id) = client
            .fetch_query::<User, QueryError>(
                key.clone(),
                CachePolicy::Ttl { ttl_ms: 5_000 },
                RequestPolicy::LatestWins,
                1_000,
                cx,
            )
            .expect("first fetch should start");

        entity.update(cx, |r, _| {
            r.complete_current_success(request_id, default_user(), 1_200)
        });

        // Normal fetch at t=2_000 would be a cache hit...
        let result = client.fetch_query::<User, QueryError>(
            key.clone(),
            CachePolicy::Ttl { ttl_ms: 5_000 },
            RequestPolicy::LatestWins,
            2_000,
            cx,
        );
        assert!(result.is_none(), "normal fetch should hit cache");

        // ...but force_fetch_query bypasses the cache
        let result = client.force_fetch_query::<User, QueryError>(
            key.clone(),
            CachePolicy::Ttl { ttl_ms: 5_000 },
            RequestPolicy::LatestWins,
            2_000,
            cx,
        );

        let (entity2, request_id2) = result.expect("force fetch should return Some");
        assert!(
            entity2.read(cx).is_loading(),
            "resource should be loading after force fetch"
        );
        assert!(
            request_id2.value() > 1,
            "force fetch should start a new request"
        );
    });
}

#[gpui::test]
fn client_fetch_query_can_complete(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from(["users", "99"]);

        // Full lifecycle: fetch_query -> complete -> verify data
        let (entity, request_id) = client
            .fetch_query::<User, QueryError>(
                key.clone(),
                CachePolicy::NoCache,
                RequestPolicy::LatestWins,
                1_000,
                cx,
            )
            .expect("fetch should start");

        assert!(entity.read(cx).is_loading());

        let success = entity.update(cx, |r, _| {
            r.complete_current_success(
                request_id,
                User {
                    id: 99,
                    name: "Dave".into(),
                },
                1_500,
            )
        });
        assert!(success, "completion should succeed");

        assert_eq!(entity.read(cx).status(), QueryStatus::Success);
        let data = entity.read(cx).data().expect("data should be present");
        assert_eq!(data.id, 99);
        assert_eq!(data.name, "Dave");
        assert!(!entity.read(cx).is_loading());
    });
}

#[gpui::test]
fn bucket_fetch_creates_and_begins(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut bucket: QueryBucket<User> = QueryBucket::new(BucketDefaults {
            cache_policy: CachePolicy::NoCache,
            request_policy: RequestPolicy::LatestWins,
            gc_time_ms: 300_000,
        });

        let key = QueryKey::from("user:new");
        assert_eq!(bucket.count(), 0, "bucket should start empty");

        let result = bucket.fetch(
            &key,
            CachePolicy::NoCache,
            RequestPolicy::LatestWins,
            1_000,
            QueryFetchMode::Normal,
            cx,
        );

        let (entity, request_id) = result.expect("fetch should create and start request");
        assert_eq!(bucket.count(), 1, "bucket should have one resource");
        assert!(entity.read(cx).is_loading());
        assert_eq!(request_id, entity.read(cx).active_request_id().unwrap());
    });
}
