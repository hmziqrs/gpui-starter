//! Client bucket coverage tests — Gaps 15, 16, 16b.
//!
//! Tests for mutation bucket type-mismatch downcast recovery and hook fallback
//! paths when QueryClient global is not registered.

use gpui::{AppContext as _, BorrowAppContext as _, Entity, TestAppContext};

use crate::client::QueryClient;
use crate::core::*;
#[allow(deprecated)]
use crate::hook::{InfiniteQueryOptions, mutate, use_infinite_query, use_mutation};
use crate::tests::test_support::*;

// -- Gap 15: MutationBucket type mismatch downcast recovery ------------------
//
// Verify that accessing the same key with different (V, T, E) types produces
// separate mutation buckets (no collision).

#[gpui::test]
fn test_mutation_bucket_type_mismatch_creates_separate_buckets(cx: &mut TestAppContext) {
    setup_query_client(cx);
    cx.update(|cx| {
        cx.update_global::<QueryClient, _>(|client, cx| {
            // Register mutations with different type triples
            let m1 = cx.new(|_| {
                MutationResource::<String, String, QueryError>::new(RetryPolicy::no_retries())
            });
            let m2 = cx.new(|_| {
                MutationResource::<u32, String, QueryError>::new(RetryPolicy::no_retries())
            });
            let m3 =
                cx.new(|_| MutationResource::<String, u32, String>::new(RetryPolicy::no_retries()));

            client.register_mutation::<String, String, QueryError>(&m1, cx);
            client.register_mutation::<u32, String, QueryError>(&m2, cx);
            client.register_mutation::<String, u32, String>(&m3, cx);

            let a = client.all_mutations::<String, String, QueryError>();
            let b = client.all_mutations::<u32, String, QueryError>();
            let c = client.all_mutations::<String, u32, String>();

            assert_eq!(a.len(), 1, "String/String/QueryError bucket should have 1");
            assert_eq!(b.len(), 1, "u32/String/QueryError bucket should have 1");
            assert_eq!(c.len(), 1, "String/u32/String bucket should have 1");
        });
    });
}

// -- Gap 16: use_infinite_query without QueryClient global (fallback path) ---
//
// The code has a fallback that creates a standalone entity, but no test
// exercises this path.

#[gpui::test]
fn test_use_infinite_query_without_query_client(cx: &mut TestAppContext) {
    // Do NOT call setup_query_client — exercise the fallback path.
    // In debug builds, use_infinite_query prints a warning but still creates
    // a standalone entity.

    struct H {
        entity: Entity<InfiniteQueryResource<Vec<i32>, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_infinite_query(
            InfiniteQueryOptions::new("no-client").cache_policy(CachePolicy::Ttl { ttl_ms: 0 }),
            |_lp| async move { Ok::<_, QueryError>((vec![1], false)) },
            cx,
        );
        H { entity }
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).entity.read(cx);
        assert_eq!(resource.status(), QueryStatus::Success);
        assert_eq!(resource.pages().len(), 1);
        assert_eq!(resource.pages()[0], vec![1]);
    });
}

// -- Gap 16b: use_mutation without QueryClient (still works) ----------------

#[gpui::test]
fn test_use_mutation_without_query_client(cx: &mut TestAppContext) {
    // Do NOT call setup_query_client.
    struct H {
        mutation: Entity<MutationResource<String, String, QueryError>>,
    }

    let harness = cx.new(|cx| {
        let (entity, _sub) = use_mutation::<String, String, QueryError, _>((), cx);
        assert_eq!(entity.read(cx).status(), MutationStatus::Idle);
        H { mutation: entity }
    });

    // Mutate should still work
    harness.update(cx, |this, cx| {
        mutate(
            &this.mutation,
            "vars".to_string(),
            |v| async move { Ok::<_, QueryError>(format!("result-{}", v)) },
            cx,
        );
    });

    cx.run_until_parked();

    cx.update(|cx| {
        let resource = harness.read(cx).mutation.read(cx);
        assert!(resource.is_success());
        assert_eq!(resource.data(), Some(&"result-vars".to_string()));
    });
}
