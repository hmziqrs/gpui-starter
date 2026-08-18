//! Deterministic GC eviction tests (integration layer).
//!
//! These tests use #[gpui::test] because they exercise QueryClient, which
//! requires a GPUI AppContext. They only need the client layer, not hooks.

use gpui::{BorrowAppContext as _, TestAppContext};
use crate::client::QueryClient;
use crate::core::*;
use crate::tests::test_support::*;

/// Helper: create a client with GC and populate a success resource with a
/// snapshot at a known timestamp. Returns the key used.
fn create_success_with_snapshot(
    client: &mut QueryClient,
    cx: &mut gpui::App,
    key: &str,
    data: &str,
    success_time_ms: u128,
    _gc_time_ms: u64,
) {
    let key = QueryKey::from(key);
    let prepared = client
        .prepare_fetch_query::<String, QueryError>(key.clone(), cx)
        .expect("should start");
    prepared.complete_success(data.to_string(), cx);

    client.update_query_snapshot::<String, QueryError>(
        &key,
        QueryStatus::Success,
        Some(success_time_ms),
        CachePolicy::Ttl { ttl_ms: 60_000 },
    );
}

#[gpui::test]
fn test_gc_evicts_exactly_expired_resources(cx: &mut TestAppContext) {
    // gc_time=1000ms. Success threshold = 2*1000 = 2000ms.
    // Create 3 resources with different snapshot ages:
    // - "young": snapshot at t=2000, GC at t=2500 => age=500 < 2000 => preserved
    // - "middle": snapshot at t=1000, GC at t=2500 => age=1500 < 2000 => preserved
    // - "old": snapshot at t=100, GC at t=2500 => age=2400 > 2000 => evicted
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            create_success_with_snapshot(client, cx, "young", "young_data", 2_000, 1_000);
            create_success_with_snapshot(client, cx, "middle", "middle_data", 1_000, 1_000);
            create_success_with_snapshot(client, cx, "old", "old_data", 100, 1_000);

            assert_eq!(client.all_queries::<String, QueryError>().len(), 3);

            client.gc_with_time(2_500, cx);

            // "young" and "middle" should survive; "old" should be evicted.
            assert_eq!(
                client.all_queries::<String, QueryError>().len(),
                2,
                "exactly 1 of 3 resources should be evicted"
            );
            assert!(
                client.query::<String, QueryError>(&QueryKey::from("young")).is_some(),
                "young (age 500ms) should survive"
            );
            assert!(
                client.query::<String, QueryError>(&QueryKey::from("middle")).is_some(),
                "middle (age 1500ms) should survive"
            );
            assert!(
                client.query::<String, QueryError>(&QueryKey::from("old")).is_none(),
                "old (age 2400ms > success_threshold 2000ms) should be evicted"
            );
        });
    });
}

#[gpui::test]
fn test_gc_eviction_counts_match(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Create 5 idle resources with no snapshot => all evicted.
            for i in 0..5 {
                let _ = client.resource::<String, QueryError>(
                    format!("idle_{}", i),
                    cx,
                );
            }
            assert_eq!(client.all_queries::<String, QueryError>().len(), 5);

            client.gc_with_time(5_000, cx);

            assert_eq!(
                client.all_queries::<String, QueryError>().len(),
                0,
                "all 5 idle resources with no snapshot should be evicted"
            );
        });
    });
}

#[gpui::test]
fn test_gc_preserves_loading_resource_with_snapshot(cx: &mut TestAppContext) {
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("loading_preserved");
            let prepared = client
                .prepare_fetch_query::<String, QueryError>(key.clone(), cx)
                .expect("should start");
            // Don't complete — leave in Loading state.

            client.update_query_snapshot::<String, QueryError>(
                &key,
                QueryStatus::LoadingEmpty,
                Some(0),
                CachePolicy::Ttl { ttl_ms: 5_000 },
            );

            // GC at t=1_000_000 — Loading resources are never evicted.
            client.gc_with_time(1_000_000, cx);

            let entity = client
                .query::<String, QueryError>(&key)
                .expect("loading resource must survive GC");

            // Now complete the fetch to verify the entity is still usable.
            prepared.complete_success("data".to_string(), cx);
            assert_eq!(
                entity.read(cx).data(),
                Some(&"data".to_string()),
                "entity should be usable after surviving GC"
            );
        });
    });
}

#[gpui::test]
fn test_gc_mixed_states_precise_eviction(cx: &mut TestAppContext) {
    // Create resources in various states and verify exact eviction counts.
    // gc_time=1000ms. Idle threshold=1000ms, Success threshold=2000ms.
    //
    // Use separate type buckets to avoid interactions between resources
    // sharing the same bucket during snapshot updates.
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Loading (with snapshot at t=0) => preserved (loading never evicted).
            let prepared = client
                .prepare_fetch_query::<String, QueryError>("loading", cx)
                .expect("should start");
            client.update_query_snapshot::<String, QueryError>(
                &QueryKey::from("loading"),
                QueryStatus::LoadingEmpty,
                Some(0),
                CachePolicy::Ttl { ttl_ms: 5_000 },
            );

            // Success (snapshot at t=1000, GC at t=2500 => age=1500 < 2000) => preserved.
            create_success_with_snapshot(client, cx, "success_fresh", "data", 1_000, 1_000);

            // Success (snapshot at t=0, GC at t=2500 => age=2500 > 2000) => evicted.
            create_success_with_snapshot(client, cx, "success_old", "data", 0, 1_000);

            assert_eq!(client.all_queries::<String, QueryError>().len(), 3);

            client.gc_with_time(2_500, cx);

            // loading + success_fresh = 2 preserved.
            // success_old = 1 evicted.
            let remaining = client.all_queries::<String, QueryError>();
            assert_eq!(
                remaining.len(),
                2,
                "exactly 1 of 3 resources should be evicted"
            );

            let remaining_keys: Vec<String> = remaining
                .iter()
                .map(|e| e.read(cx).key().to_path())
                .collect();
            assert!(
                remaining_keys.contains(&"loading".to_string()),
                "loading should survive: {:?}",
                remaining_keys
            );
            assert!(
                remaining_keys.contains(&"success_fresh".to_string()),
                "success_fresh should survive: {:?}",
                remaining_keys
            );

            // Clean up: complete the loading fetch.
            prepared.complete_success("data".to_string(), cx);
        });
    });
}

#[gpui::test]
fn test_gc_survive_then_evict_after_threshold_crossed(cx: &mut TestAppContext) {
    // Same resource survives GC at time T1, then gets evicted at T2.
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("aged");
            create_success_with_snapshot(client, cx, "aged", "data", 1_000, 1_000);

            // GC at t=2000: age=1000 < success_threshold(2000) => preserved.
            client.gc_with_time(2_000, cx);
            assert!(
                client.query::<String, QueryError>(&key).is_some(),
                "age=1000ms < success_threshold=2000ms => should survive"
            );

            // GC at t=3500: age=2500 > success_threshold(2000) => evicted.
            client.gc_with_time(3_500, cx);
            assert!(
                client.query::<String, QueryError>(&key).is_none(),
                "age=2500ms > success_threshold=2000ms => should be evicted"
            );
        });
    });
}

#[gpui::test]
fn test_gc_boundary_success_threshold_exact(cx: &mut TestAppContext) {
    // Test the exact boundary: age == success_threshold.
    // gc_time=1000 => success_threshold=2000.
    // Snapshot at t=1000, GC at t=3000 => age=2000 == success_threshold.
    //
    // GC uses `age_ms < success_threshold` to retain (line ~437 in bucket.rs).
    // When age == threshold, the condition is false → evicted (>= semantics).
    setup_query_client_with_gc(cx, 1_000);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            let key = QueryKey::from("boundary");
            create_success_with_snapshot(client, cx, "boundary", "data", 1_000, 1_000);

            // GC at t=3000: age=3000-1000=2000 == success_threshold => evicted.
            client.gc_with_time(3_000, cx);
            assert!(
                client.query::<String, QueryError>(&key).is_none(),
                "age=2000ms == success_threshold=2000ms => must be evicted (>= boundary)"
            );
        });
    });
}
