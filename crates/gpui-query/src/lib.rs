//! gpui-query — Zero-boilerplate async state management for GPUI.
//!
//! Inspired by [TanStack Query v5](https://tanstack.com/query), redesigned for
//! GPUI's synchronous rendering model and Rust's ownership semantics.
//!
//! # v2 Improvements over v1
//!
//! - **Options-first API** with sensible defaults — `use_query("users", fetcher, cx)`
//! - **Tuple return types** — hooks return `(Entity<QueryResource<T,E>>, Subscription)` for explicit control
//! - **Signal-always** — fetchers always receive `QuerySignal` for cooperative cancellation
//! - **Fixed signal lifecycle** — signals are cancelled on LatestWins replacement
//! - **Fixed retry loops** — single counter, signal checked between attempts
//! - **Efficient re-renders** — `cx.notify()` only on terminal state changes
//! - **`QueryError: Display + Error`** — ecosystem interop with `?` and `anyhow`
//! - **`AHashMap`** for trusted cache keys — ~2x faster lookups
//! - **Bounded `max_pages`** default — prevents unbounded memory growth
//! - **Actual mutation GC** — no more memory leaks
//!
//! # Layers
//!
//! - **`core`** — Serde-only state machine (`QueryResource`, `CachePolicy`, etc.)
//! - **`client`** — GPUI `QueryClient` registry with type-partitioned buckets
//! - **`hook`** — `use_query()` / `use_mutation()` / `use_infinite_query()` hooks
//!
//! # Quick Start
//!
//! Hooks are re-exported at the crate root for ergonomic imports:
//!
//! ```ignore
//! use gpui_query::{use_query, use_mutation, use_infinite_query, QueryClient};
//! ```

#[cfg(feature = "core")]
pub mod core;

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "hook")]
pub mod hook;

// Convenience re-exports (star-export each enabled layer at crate root).
//
// `current_time_ms` is defined in both `client` and `hook` modules with
// identical implementations. Both glob re-exports are intentional so that
// users can import from either layer. Suppress the ambiguous_glob_reexports
// lint since the duplicate is harmless and both are public API.
#[cfg(feature = "core")]
pub use core::*;

#[cfg(feature = "client")]
#[allow(ambiguous_glob_reexports)]
pub use client::*;

#[cfg(feature = "hook")]
pub use hook::*;

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
