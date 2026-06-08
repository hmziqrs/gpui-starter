//! Integration tests for QueryBucket operations.

use gpui::TestAppContext;

use crate::client::{BucketDefaults, QueryBucket, QueryBucketTrait};
use crate::core::*;
use crate::integration_client_fixtures::*;

#[gpui::test]
fn bucket_creates_and_deduplicates_resources(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut bucket: QueryBucket<User> = QueryBucket::new(BucketDefaults {
            cache_policy: CachePolicy::Ttl { ttl_ms: 60_000 },
            request_policy: RequestPolicy::LatestWins,
            gc_time_ms: 300_000,
        });

        let key = QueryKey::from("user:1");
        let e1 = bucket.resource(key.clone(), cx);
        assert_eq!(bucket.count(), 1);

        // Same key returns the same entity
        let e2 = bucket.resource(key.clone(), cx);
        assert_eq!(bucket.count(), 1);
        assert_eq!(e1.entity_id(), e2.entity_id());

        // Different key creates a new entity
        let e3 = bucket.resource(QueryKey::from("user:2"), cx);
        assert_eq!(bucket.count(), 2);
        assert_ne!(e1.entity_id(), e3.entity_id());
    });
}

#[gpui::test]
fn bucket_begin_request_for_starts_loading(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut bucket: QueryBucket<User> = QueryBucket::new(BucketDefaults {
            cache_policy: CachePolicy::NoCache,
            request_policy: RequestPolicy::LatestWins,
            gc_time_ms: 300_000,
        });

        let key = QueryKey::from("user:1");
        bucket.resource(key.clone(), cx);

        let result = bucket.begin_request_for(&key, 1_000, QueryFetchMode::Normal, cx);
        assert!(matches!(result, Some(QueryBeginResult::Started { .. })));

        // Verify the entity is loading
        let entity = bucket.resources.get(&key).unwrap();
        assert!(entity.read(cx).is_loading());
    });
}

#[gpui::test]
fn bucket_begin_request_for_unknown_key_returns_none(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut bucket: QueryBucket<User> = QueryBucket::new(BucketDefaults {
            cache_policy: CachePolicy::NoCache,
            request_policy: RequestPolicy::LatestWins,
            gc_time_ms: 300_000,
        });

        let result = bucket.begin_request_for(
            &QueryKey::from("nonexistent"),
            1_000,
            QueryFetchMode::Normal,
            cx,
        );
        assert!(result.is_none());
    });
}

#[gpui::test]
fn bucket_gc_removes_stale_idle_resources(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut bucket: QueryBucket<User> = QueryBucket::new(BucketDefaults {
            cache_policy: CachePolicy::NoCache,
            request_policy: RequestPolicy::LatestWins,
            gc_time_ms: 1_000, // 1 second GC time
        });

        // Create resource with stale data
        let stale_key = QueryKey::from("stale_user");
        let entity = bucket.resource(stale_key.clone(), cx);
        entity.update(cx, |r, _| r.apply_success(default_user(), 100));
        assert_eq!(bucket.count(), 1);

        // GC at t=2000: age = 1900 > 1000 → collected
        bucket.gc(cx, 2_000, 1_000);
        assert_eq!(bucket.count(), 0);
    });
}

#[gpui::test]
fn bucket_gc_preserves_fresh_resources(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut bucket: QueryBucket<User> = QueryBucket::new(BucketDefaults {
            cache_policy: CachePolicy::NoCache,
            request_policy: RequestPolicy::LatestWins,
            gc_time_ms: 1_000,
        });

        let entity = bucket.resource(QueryKey::from("fresh_user"), cx);
        entity.update(cx, |r, _| r.apply_success(default_user(), 1_500));

        // GC at t=2000: age = 500 < 1000 → kept
        bucket.gc(cx, 2_000, 1_000);
        assert_eq!(bucket.count(), 1);
    });
}

#[gpui::test]
fn bucket_gc_preserves_resources_with_active_requests(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut bucket: QueryBucket<User> = QueryBucket::new(BucketDefaults {
            cache_policy: CachePolicy::NoCache,
            request_policy: RequestPolicy::LatestWins,
            gc_time_ms: 1_000,
        });

        let key = QueryKey::from("loading_user");
        bucket.resource(key.clone(), cx);
        bucket.begin_request_for(&key, 1_000, QueryFetchMode::Normal, cx);

        // GC at t=10000: resource is old but has an active request → kept
        bucket.gc(cx, 10_000, 1_000);
        assert_eq!(bucket.count(), 1);
    });
}

#[gpui::test]
fn bucket_invalidate_matching_uses_prefix_filter(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut bucket: QueryBucket<User> = QueryBucket::new(BucketDefaults {
            cache_policy: CachePolicy::Ttl { ttl_ms: 60_000 },
            request_policy: RequestPolicy::LatestWins,
            gc_time_ms: 300_000,
        });

        let u1_key = QueryKey::from(["users", "1"]);
        let u2_key = QueryKey::from(["users", "2"]);
        let u3_key = QueryKey::from(["admins", "1"]);

        let e1 = bucket.resource(u1_key.clone(), cx);
        let e2 = bucket.resource(u2_key.clone(), cx);
        let _ = bucket.resource(u3_key.clone(), cx);

        // Populate with cached data
        e1.update(cx, |r, _| r.apply_success(default_user(), 1_000));
        e2.update(cx, |r, _| {
            r.apply_success(
                User {
                    id: 2,
                    name: "Bob".into(),
                },
                1_000,
            )
        });

        // Invalidate all "users" keys
        let prefix = QueryKey::from(["users"]);
        bucket.invalidate_matching(&QueryKeyFilter::Prefix(&prefix), cx);

        // User resources: cache expired, data still present
        let e1 = bucket.resources.get(&u1_key).unwrap();
        assert!(
            e1.read(cx).data().is_some(),
            "data should remain after invalidate"
        );
        assert!(
            !e1.read(cx).is_cache_fresh(1_500),
            "cache should be stale after invalidate"
        );

        let e2 = bucket.resources.get(&u2_key).unwrap();
        assert!(!e2.read(cx).is_cache_fresh(1_500));

        // Admin resource: unaffected
        let e3 = bucket.resources.get(&u3_key).unwrap();
        // No data was set for admins, so cache was never fresh
        assert!(!e3.read(cx).is_cache_fresh(1_500));
    });
}

#[gpui::test]
fn bucket_reset_matching_clears_all_state(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut bucket: QueryBucket<User> = QueryBucket::new(BucketDefaults {
            cache_policy: CachePolicy::Ttl { ttl_ms: 60_000 },
            request_policy: RequestPolicy::LatestWins,
            gc_time_ms: 300_000,
        });

        let key = QueryKey::from("user:1");
        let entity = bucket.resource(key.clone(), cx);
        entity.update(cx, |r, _| r.apply_success(default_user(), 1_000));

        assert!(entity.read(cx).data().is_some());

        bucket.reset_matching(&QueryKeyFilter::All, cx);

        assert!(entity.read(cx).data().is_none());
        assert_eq!(entity.read(cx).status(), QueryStatus::Idle);
    });
}
