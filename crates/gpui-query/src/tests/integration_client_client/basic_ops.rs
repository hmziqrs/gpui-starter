use gpui::TestAppContext;

use crate::client::QueryClient;
use crate::core::*;
use crate::integration_client_fixtures::*;

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
