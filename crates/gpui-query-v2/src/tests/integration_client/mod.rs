//! Integration tests for the QueryClient layer (v2).
//!
//! Tests use `#[gpui::test]` with `TestAppContext` and the `test_support` helpers.
//! They exercise the full client API: resource creation, type partitioning,
//! invalidation, reset, GC, mutations, diagnostics, signals, data access,
//! and observers.
//!
//! # Context pattern
//!
//! All tests use `cx.update_global::<QueryClient, _>(|client, cx| ...)` to
//! get `(&mut QueryClient, &mut App)`. Methods like `resource()` require
//! `&mut self` and `&mut App`, so `cx.global()` (immutable) cannot be used.
//!
//! # GC test design
//!
//! The bucket's GC reads a cached `StatusSnapshot` (not the live entity).
//! Direct entity manipulation (`apply_success`, etc.) and `PreparedFetch`
//! completions update the entity but do NOT update the bucket snapshot.
//! The snapshot is only updated by the hook layer in production.
//!
//! For deterministic GC tests, we use `client.update_query_snapshot()` to
//! simulate what the hook layer would do: set the snapshot status and
//! `last_updated_ms` to known values. This lets us assert exact eviction
//! and preservation behavior without the hook layer.

mod client_basics;
mod data_access;
mod invalidation_reset_gc;
mod mutations_lifecycle;
