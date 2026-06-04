//! Integration tests for the Client layer using GPUI's `TestAppContext`.

use gpui::TestAppContext;

use crate::client::{BucketDefaults, QueryBucket, QueryBucketTrait, QueryClient};
use crate::core::*;

// ── Test fixtures ──────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
struct User {
    id: u32,
    name: String,
}

#[derive(Clone, Debug, PartialEq)]
struct Post {
    id: u32,
    title: String,
}

fn default_user() -> User {
    User {
        id: 1,
        name: "Alice".into(),
    }
}

fn default_post() -> Post {
    Post {
        id: 1,
        title: "Hello World".into(),
    }
}

// ── QueryBucket tests ──────────────────────────────────────────────────

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

// ── QueryClient tests ──────────────────────────────────────────────────

#[gpui::test]
fn client_stores_multiple_types_in_separate_buckets(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let user = client.resource::<User, QueryError>(QueryKey::from(["users", "1"]), cx);
        let post = client.resource::<Post, QueryError>(QueryKey::from(["posts", "1"]), cx);

        assert_eq!(client.bucket_count(), 2);
        assert_eq!(client.total_count(), 2);
        assert_ne!(user.entity_id(), post.entity_id());
    });
}

#[gpui::test]
fn client_deduplicates_same_key_same_type(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from(["users", "1"]);
        let e1 = client.resource::<User, QueryError>(key.clone(), cx);
        let e2 = client.resource::<User, QueryError>(key.clone(), cx);

        assert_eq!(client.total_count(), 1);
        assert_eq!(e1.entity_id(), e2.entity_id());
    });
}

#[gpui::test]
fn client_invalidate_queries_prefix_match_across_types(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        );

        let u1 = client.resource::<User, QueryError>(QueryKey::from(["users", "1"]), cx);
        let u2 = client.resource::<User, QueryError>(QueryKey::from(["users", "2"]), cx);
        let p1 = client.resource::<Post, QueryError>(QueryKey::from(["posts", "1"]), cx);

        u1.update(cx, |r, _| r.apply_success(default_user(), 1_000));
        u2.update(cx, |r, _| {
            r.apply_success(
                User {
                    id: 2,
                    name: "Bob".into(),
                },
                1_000,
            )
        });
        p1.update(cx, |r, _| r.apply_success(default_post(), 1_000));

        // Invalidate all "users" — posts unaffected
        let prefix = QueryKey::from(["users"]);
        client.invalidate_queries(&QueryKeyFilter::Prefix(&prefix), cx);

        assert!(
            !u1.read(cx).is_cache_fresh(1_500),
            "user:1 cache should be stale"
        );
        assert!(
            !u2.read(cx).is_cache_fresh(1_500),
            "user:2 cache should be stale"
        );
        assert!(
            p1.read(cx).is_cache_fresh(1_500),
            "posts:1 cache should still be fresh"
        );
    });
}

#[gpui::test]
fn client_invalidate_queries_all_matches_everything(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        );

        let u = client.resource::<User, QueryError>(QueryKey::from("user"), cx);
        let p = client.resource::<Post, QueryError>(QueryKey::from("post"), cx);

        u.update(cx, |r, _| r.apply_success(default_user(), 1_000));
        p.update(cx, |r, _| r.apply_success(default_post(), 1_000));

        client.invalidate_queries(&QueryKeyFilter::All, cx);

        assert!(!u.read(cx).is_cache_fresh(1_500));
        assert!(!p.read(cx).is_cache_fresh(1_500));
    });
}

#[gpui::test]
fn client_gc_removes_stale_across_types(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client =
            QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins).with_gc_time(1_000);

        let u = client.resource::<User, QueryError>(QueryKey::from("old_user"), cx);
        u.update(cx, |r, _| r.apply_success(default_user(), 100));

        let p = client.resource::<Post, QueryError>(QueryKey::from("fresh_post"), cx);
        p.update(cx, |r, _| r.apply_success(default_post(), 1_800));

        assert_eq!(client.total_count(), 2);

        // GC at t=2000: user age=1900 > 1000 (collected), post age=200 < 1000 (kept)
        client.gc(cx, 2_000);
        assert_eq!(client.total_count(), 1);

        // Verify the survivor is the post
        assert!(
            client.contains::<Post, QueryError>(&QueryKey::from("fresh_post")),
            "fresh post should survive GC"
        );
        assert!(
            !client.contains::<User, QueryError>(&QueryKey::from("old_user")),
            "stale user should be collected"
        );
    });
}

#[gpui::test]
fn client_reset_queries_clears_state(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        );

        let u = client.resource::<User, QueryError>(QueryKey::from("user:1"), cx);
        u.update(cx, |r, _| r.apply_success(default_user(), 1_000));

        assert!(u.read(cx).data().is_some());

        client.reset_queries(&QueryKeyFilter::All, cx);

        assert!(u.read(cx).data().is_none());
        assert_eq!(u.read(cx).status(), QueryStatus::Idle);
    });
}

// ── Full lifecycle test ────────────────────────────────────────────────

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

// ── QueryClient cancel_query / signal_for tests ────────────────────────

#[gpui::test]
fn client_cancel_query_cancels_active_request(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from(["users", "42"]);
        let entity = client.resource::<User, QueryError>(key.clone(), cx);

        // Start a request
        let sequencer = &mut RequestSequencer::new();
        entity.update(cx, |r, _| {
            let result = r.begin_request(sequencer, 1_000, QueryFetchMode::Normal);
            assert!(matches!(result, QueryBeginResult::Started { .. }));
        });

        // Grab the signal before cancelling
        let signal = client.signal_for::<User, QueryError>(&key, cx);
        assert!(signal.is_some(), "signal should exist while loading");
        let signal = signal.unwrap();
        assert!(!signal.is_cancelled());

        // Cancel via client
        let cancelled =
            client.cancel_query::<User, QueryError>(&key, QueryError::cancelled("aborted"), cx);
        assert!(cancelled, "should have cancelled an active request");
        assert_eq!(entity.read(cx).status(), QueryStatus::Cancelled);
        assert!(signal.is_cancelled(), "signal should be cancelled");
    });
}

#[gpui::test]
fn client_cancel_query_returns_false_for_idle_resource(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from(["users", "99"]);
        let _entity = client.resource::<User, QueryError>(key.clone(), cx);

        // Resource is idle (no request started)
        let cancelled =
            client.cancel_query::<User, QueryError>(&key, QueryError::cancelled("nope"), cx);
        assert!(!cancelled, "idle resource should not be cancellable");
    });
}

#[gpui::test]
fn client_cancel_query_returns_false_for_nonexistent_key(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let cancelled = client.cancel_query::<User, QueryError>(
            &QueryKey::from("ghost"),
            QueryError::cancelled("nope"),
            cx,
        );
        assert!(!cancelled, "nonexistent key should not be cancellable");
    });
}

#[gpui::test]
fn client_signal_for_returns_none_when_no_active_request(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from(["users", "1"]);
        let _entity = client.resource::<User, QueryError>(key.clone(), cx);

        // Resource is idle, no signal
        let signal = client.signal_for::<User, QueryError>(&key, cx);
        assert!(signal.is_none(), "no signal for idle resource");
    });
}

#[gpui::test]
fn client_signal_for_returns_signal_while_loading(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(CachePolicy::NoCache, RequestPolicy::LatestWins);

        let key = QueryKey::from(["users", "7"]);
        let entity = client.resource::<User, QueryError>(key.clone(), cx);

        let sequencer = &mut RequestSequencer::new();
        entity.update(cx, |r, _| {
            let _ = r.begin_request(sequencer, 1_000, QueryFetchMode::Normal);
        });

        let signal = client.signal_for::<User, QueryError>(&key, cx);
        assert!(signal.is_some(), "signal should exist while loading");
        assert!(!signal.unwrap().is_cancelled());
    });
}

// ── fetch_query / force_fetch_query tests ────────────────────────────────

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

// ── Client remove_queries / clear tests ─────────────────────────────────────

#[gpui::test]
fn test_client_remove_queries(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        );

        let u1 = client.resource::<User, QueryError>(QueryKey::from(["users", "1"]), cx);
        let u2 = client.resource::<User, QueryError>(QueryKey::from(["users", "2"]), cx);
        let p1 = client.resource::<Post, QueryError>(QueryKey::from(["posts", "1"]), cx);

        u1.update(cx, |r, _| r.apply_success(default_user(), 1_000));
        u2.update(cx, |r, _| {
            r.apply_success(
                User {
                    id: 2,
                    name: "Bob".into(),
                },
                1_000,
            )
        });
        p1.update(cx, |r, _| r.apply_success(default_post(), 1_000));

        assert_eq!(client.total_count(), 3);

        // Remove all "users" resources
        let prefix = QueryKey::from(["users"]);
        client.remove_queries(&QueryKeyFilter::Prefix(&prefix), cx);

        assert_eq!(client.total_count(), 1, "only post should remain");
        assert!(
            !client.contains::<User, QueryError>(&QueryKey::from(["users", "1"])),
            "user:1 should be removed"
        );
        assert!(
            !client.contains::<User, QueryError>(&QueryKey::from(["users", "2"])),
            "user:2 should be removed"
        );
        assert!(
            client.contains::<Post, QueryError>(&QueryKey::from(["posts", "1"])),
            "posts:1 should still exist"
        );
    });
}

#[gpui::test]
fn test_client_clear(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut client = QueryClient::new(
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
        );

        let _u = client.resource::<User, QueryError>(QueryKey::from("user:1"), cx);
        let _p = client.resource::<Post, QueryError>(QueryKey::from("post:1"), cx);
        let _m = client.mutation_resource::<String, User, QueryError>(
            &QueryKey::from("mutation:1"),
            cx,
        );

        assert_eq!(client.total_count(), 2);
        assert_eq!(client.mutation_count(), 1);

        client.clear();

        assert_eq!(client.total_count(), 0, "queries should be cleared");
        assert_eq!(client.mutation_count(), 0, "mutations should be cleared");
        assert_eq!(client.bucket_count(), 0, "buckets should be cleared");
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
