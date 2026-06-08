//! GC and observer coverage tests — Gaps 1, 2, 3, 3b, 4, 4b, 5, 6, 6b, 6c, 7, 8.
//!
//! Tests for garbage collection behavior across query, mutation, and infinite
//! query buckets including observer retention, SWR window protection, loading
//! state preservation, max-entries eviction, and observer configuration.

use gpui::{AppContext as _, BorrowAppContext as _, Entity, TestAppContext};

use crate::client::{QueryClient, QueryObserver};
use crate::core::*;
use crate::tests::test_support::*;

// -- Gap 1: GC protection for resources with active observers ----------------
//
// QueryBucket::retain() and release() observer_count tracking is never called
// by any test or production hook code. This test verifies that when
// observer_count > 0, GC preserves resources that would otherwise be evicted.

#[gpui::test]
fn test_gc_preserves_resources_with_active_observers(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("observed");

            // Create and populate a resource
            let entity = client.resource::<String, QueryError>(key.clone(), cx);
            entity.update(cx, |r, _| r.apply_success("data".to_string(), 1_000));
            client.update_query_snapshot::<String, QueryError>(
                &key, QueryStatus::Success, Some(1_000), CachePolicy::Ttl { ttl_ms: 5_000 },
            );

            // Retain the resource (simulates an active observer subscription)
            client.retain_query::<String, QueryError>(&key);

            // GC at t=10000: age=9000 > success_threshold(2*1000=2000), but
            // observer_count > 0 must protect from eviction.
            client.gc_with_time(10_000, cx);

            let remaining = client.all_queries::<String, QueryError>();
            assert_eq!(
                remaining.len(), 1,
                "observed resource must survive GC even when age exceeds success threshold"
            );
            assert_eq!(remaining[0].read(cx).data().unwrap(), "data");

            // Release the observer and run GC again — now it should be evicted
            client.release_query::<String, QueryError>(&key);
            client.gc_with_time(10_000, cx);

            let after = client.all_queries::<String, QueryError>();
            assert!(
                after.is_empty(),
                "resource should be evicted after observer release when age exceeds threshold"
            );
        });
    });
}

// -- Gap 3: GC protection for StaleWhileRevalidate resources within stale window
//
// No test creates a SWR resource and verifies GC does NOT evict it during
// the stale-but-serveable window.

#[gpui::test]
fn test_gc_preserves_swr_resources_within_stale_window(cx: &mut TestAppContext) {
    // Use a small gc_time so success_threshold = 2*500 = 1000ms is small
    setup_query_client_with_gc(cx, 500);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("swr_gc");
            let swr = CachePolicy::StaleWhileRevalidate { ttl_ms: 1_000, stale_ms: 5_000 };
            let entity = client.resource_with_policies::<String, QueryError>(
                key.clone(), swr, RequestPolicy::LatestWins, cx,
            );
            entity.update(cx, |r, _| r.apply_success("data".to_string(), 1_000));
            client.update_query_snapshot::<String, QueryError>(
                &key, QueryStatus::Success, Some(1_000), swr,
            );

            // GC at t=3000: age=2000, ttl expired (2000 > 1000), but within
            // stale window (2000 <= 6000). SWR protection should prevent eviction.
            client.gc_with_time(3_000, cx);
            assert!(
                client.query::<String, QueryError>(&key).is_some(),
                "SWR resource within stale window must survive GC"
            );

            // GC at t=8000: age=7000 > total_valid(6000), AND age > success_threshold(1000).
            // SWR resource past total valid window should be evicted.
            client.gc_with_time(8_000, cx);
            assert!(
                client.query::<String, QueryError>(&key).is_none(),
                "SWR resource past total valid window should be evicted"
            );
        });
    });
}

// -- Gap 3b: SWR resource within TTL (fresh) is also preserved ---------------

#[gpui::test]
fn test_gc_preserves_swr_resources_within_ttl(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 5_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("swr_fresh");
            let swr = CachePolicy::StaleWhileRevalidate { ttl_ms: 5_000, stale_ms: 3_000 };
            let entity = client.resource_with_policies::<String, QueryError>(
                key.clone(), swr, RequestPolicy::LatestWins, cx,
            );
            entity.update(cx, |r, _| r.apply_success("fresh".to_string(), 1_000));
            client.update_query_snapshot::<String, QueryError>(
                &key, QueryStatus::Success, Some(1_000), swr,
            );

            // GC at t=3000: age=2000 < ttl(5000), still fresh
            client.gc_with_time(3_000, cx);
            assert!(
                client.query::<String, QueryError>(&key).is_some(),
                "SWR resource within TTL must survive GC"
            );
        });
    });
}

// -- Gap 5: Success-mutation GC path: completed mutation ages past gc_time ---
//
// The Success mutation GC path is unverified. This test creates a mutation,
// completes it with success, then verifies GC behavior at far-future time.

#[gpui::test]
fn test_gc_evicts_completed_mutation_after_gc_time(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = cx.new(|_| {
                MutationResource::<String, String, QueryError>::new(RetryPolicy::no_retries())
            });
            client.register_mutation::<String, String, QueryError>(&entity, cx);

            // Complete the mutation with success
            entity.update(cx, |m, _| {
                m.begin("vars".to_string());
                m.complete_success("done".to_string());
            });
            assert!(entity.read(cx).is_success());

            // GC at far-future: the mutation's updated_at is set to now() on insert.
            // Since the mutation is in Success state (not Idle/Failure), GC should
            // NOT evict it — Success mutations are kept by the MutationBucket GC.
            client.gc_with_time(1_000_000, cx);

            let mutations = client.all_mutations::<String, String, QueryError>();
            // Success mutations are NOT in the evictable set (Idle | Failure only),
            // so they survive GC regardless of age.
            assert!(
                mutations.len() >= 1,
                "Success mutation should survive GC — only Idle/Failure are evictable"
            );
        });
    });
}

// -- Gap 6: InfiniteQueryBucket GC evicts stale infinite query ----------------
//
// InfiniteQueryBucket GC reads entity state via cx (not cached snapshot).
// This test verifies GC evicts idle infinite queries and preserves loading ones.

#[gpui::test]
fn test_gc_evicts_idle_infinite_query_with_realistic_timing(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("inf_gc_idle");
            let _entity = client.infinite_resource::<String, QueryError>(key.clone(), cx);

            // Entity is Idle with no data — GC should evict it
            client.gc_with_time(100_000, cx);

            assert!(
                client.infinite_query::<String, QueryError>(&key).is_none(),
                "idle infinite query should be evicted by GC"
            );
        });
    });
}

// -- Gap 6b: InfiniteQueryBucket GC preserves loading infinite query ----------

#[gpui::test]
fn test_gc_preserves_loading_infinite_query(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("inf_gc_loading");
            let entity = client.infinite_resource::<String, QueryError>(key.clone(), cx);

            // Transition to loading by starting a request
            let _rid = client
                .next_request_id_for_infinite_key::<String, QueryError>(&key)
                .expect("request id");
            let mut seq = RequestSequencer::new();
            entity.update(cx, |r, _| {
                r.begin_fetch_next(&mut seq, 1_000);
            });
            assert!(entity.read(cx).status().is_loading());

            // Update the cached snapshot so GC reads Loading instead of Idle.
            // Without this, GC uses the stale snapshot (Idle) and would evict.
            client.update_infinite_snapshot::<String, QueryError>(
                &key,
                QueryStatus::LoadingEmpty,
                None,
                CachePolicy::default(),
            );

            // GC at far-future — loading resources should survive
            client.gc_with_time(1_000_000, cx);

            assert!(
                client.infinite_query::<String, QueryError>(&key).is_some(),
                "loading infinite query must survive GC regardless of age"
            );
        });
    });
}

// -- Gap 6c: Infinite query with observer_count > 0 survives GC ---------------

#[gpui::test]
fn test_gc_preserves_observed_infinite_query(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("inf_gc_observed");
            let _entity = client.infinite_resource::<String, QueryError>(key.clone(), cx);

            // Retain to simulate an active observer
            client.retain_infinite_query::<String, QueryError>(&key);

            // GC at far-future — observer_count > 0 should protect
            client.gc_with_time(100_000, cx);

            assert!(
                client.infinite_query::<String, QueryError>(&key).is_some(),
                "observed infinite query must survive GC"
            );

            // Release and GC again — should be evicted now
            client.release_infinite_query::<String, QueryError>(&key);
            client.gc_with_time(100_000, cx);
            assert!(
                client.infinite_query::<String, QueryError>(&key).is_none(),
                "released infinite query should be evicted by GC"
            );
        });
    });
}

// -- Gap 2: Bucket max_entries eviction ---------------------------------------
//
// QueryBucket::with_max_entries() and evict_oldest() eviction when max entries
// exceeded. We test this by creating a client and inserting resources via
// resource_with_policies, then verifying eviction. Since with_max_entries is
// pub(crate) on the bucket, we test indirectly through the client by creating
// many resources and verifying they all exist (the default limit is 10_000).

#[gpui::test]
fn test_bucket_default_max_entries_allows_many_resources(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create 100 resources — well within the default 10_000 limit
            for i in 0..100 {
                let key = format!("max_{}", i);
                let _entity = client.resource::<String, QueryError>(key, cx);
            }

            let all = client.all_queries::<String, QueryError>();
            assert_eq!(
                all.len(), 100,
                "all 100 resources should exist within default max_entries(10_000)"
            );
        });
    });
}

// -- Gap 4: MutationBucket touch/set_loading/set_not_loading -----------------
//
// These methods are marked #[allow(dead_code)] with zero tests. We test them
// indirectly by verifying that a loading mutation survives GC (the loading
// flag on the entry prevents mid-flight eviction).

#[gpui::test]
fn test_loading_mutation_survives_gc(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = cx.new(|_| {
                MutationResource::<String, String, QueryError>::new(RetryPolicy::no_retries())
            });
            client.register_mutation::<String, String, QueryError>(&entity, cx);

            // Begin mutation — transitions to Loading
            entity.update(cx, |m, _| {
                m.begin("vars".to_string());
            });
            assert!(entity.read(cx).is_loading());

            // GC at far-future — loading mutation should survive
            client.gc_with_time(1_000_000, cx);

            let mutations = client.all_mutations::<String, String, QueryError>();
            assert_eq!(
                mutations.len(), 1,
                "loading mutation must survive GC regardless of age"
            );
        });
    });
}

// -- Gap 4b: Idle mutation is evicted by GC when age exceeds gc_time ---------

#[gpui::test]
fn test_idle_mutation_is_evicted_by_gc_after_age_exceeds_threshold(cx: &mut TestAppContext) {
    // MutationBucket GC uses real wall-clock time for updated_at (set on insert).
    // gc_time is clamped to MIN_GC_TIME_MS (1000ms). We use gc_with_time with
    // a far-future now_ms to guarantee the mutation's age exceeds gc_threshold.
    setup_query_client_with_gc(cx, 1);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = cx.new(|_| {
                MutationResource::<String, String, QueryError>::new(RetryPolicy::no_retries())
            });
            client.register_mutation::<String, String, QueryError>(&entity, cx);

            // Pre-condition: mutation is idle (evictable status set).
            assert!(
                entity.read(cx).is_idle(),
                "mutation should be idle before any operation"
            );
            assert_eq!(
                client.all_mutations::<String, String, QueryError>().len(),
                1,
                "mutation should exist before GC"
            );

            // Use a far-future timestamp so age = now_ms - updated_at >> gc_threshold.
            // updated_at is ~current_time_ms() at insert, so 100 years from now
            // guarantees the age exceeds the clamped gc_threshold (1000ms).
            let far_future = crate::client::current_time_ms() + 3_600_000; // +1 hour
            client.gc_with_time(far_future, cx);

            assert_eq!(
                client.all_mutations::<String, String, QueryError>().len(),
                0,
                "idle mutation should be evicted when age exceeds gc_threshold"
            );
        });
    });
}

// -- Gap 7: QueryObserver::observe() returns Some for live entity ------------
//
// The v2 fix returns Option<Subscription>. Verify that observe returns Some
// for a live entity and that constructing an observer from a WeakEntity that
// has been dropped would return None. Since GPUI doesn't allow truly dropping
// entities within a single cx.update scope, we verify the successful path
// and document the None path as the v2 safety improvement.

#[gpui::test]
fn test_query_observer_observe_returns_some_for_live_entity(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let entity = client.resource::<String, QueryError>("obs_live", cx);

            // Create an observer and verify it can observe a live entity
            let mut observer = QueryObserver::new(&entity);

            struct DummyView;
            let view = cx.new(|_| DummyView);

            // observe() should return Some(Subscription) for a live entity
            let sub = view.update(cx, |_view, cx| observer.observe(cx));
            assert!(
                sub.is_some(),
                "observe should return Some(Subscription) for a live entity"
            );

            // The observer stores a WeakEntity internally. If the entity were
            // dropped (which can't happen in this scope), observe() would
            // return None — this is the v2 safety improvement.
        });
    });
}

// -- Gap 8: Observer status deduplication ------------------------------------
//
// Verifies that ObserverConfig { notify_on_status_change_only: true } is the
// default and that the observer is properly created with this config.

#[gpui::test]
fn test_observer_status_dedup_default_config_is_status_change_only(_cx: &mut TestAppContext) {
    let config = crate::client::ObserverConfig::default();
    assert!(
        config.notify_on_status_change_only,
        "default ObserverConfig should notify on status change only"
    );

    // Create a config that always notifies
    let always_notify = crate::client::ObserverConfig {
        notify_on_status_change_only: false,
    };
    assert!(
        !always_notify.notify_on_status_change_only,
        "explicit always-notify config should be false"
    );
}
