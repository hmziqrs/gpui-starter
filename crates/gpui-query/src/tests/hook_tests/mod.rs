//! Hook-layer integration tests for gpui-query.
//!
//! Tests use `#[gpui::test]` with `TestAppContext` to exercise the full
//! hook pipeline: entity creation, observation subscription, fetch spawning,
//! completion, and lifecycle management.
//!
//! # Context pattern
//!
//! Hook functions require `&mut Context<C>` (a component-typed context), not
//! `&mut App`. We create harness entities via `cx.new(|cx| ...)` which provides
//! `Context<Harness>`. For post-creation hook calls (e.g. `fetch_query`, `mutate`),
//! we use `harness.update(cx, |_, cx| ...)`. Harness structs store entity handles
//! so they can be inspected after async work completes.

mod infinite_query_tests;
mod mutation_tests;
mod query_tests;
