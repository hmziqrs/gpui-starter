//! gpui-query — Transport-agnostic query lifecycle primitives for GPUI.
//!
//! Inspired by [TanStack Query](https://tanstack.com/query), adapted for
//! GPUI's synchronous rendering model and Rust's ownership semantics.
//!
//! # Layers
//!
//! - **`core`** — Serde-only state machine (`QueryResource`, `CachePolicy`, etc.)
//! - **`client`** — GPUI `QueryClient` registry with type-partitioned buckets
//! - **`hook`** — `use_query()` ergonomic hook for components

#[cfg(feature = "core")]
pub mod core;

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "hook")]
pub mod hook;

// Convenience re-exports from core (always available when core is enabled)
#[cfg(feature = "core")]
pub use core::*;

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "tests/test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "tests/core_cache.rs"]
mod core_cache;

#[cfg(test)]
#[path = "tests/core_lifecycle.rs"]
mod core_lifecycle;

#[cfg(test)]
#[path = "tests/core_request.rs"]
mod core_request;

#[cfg(test)]
#[path = "tests/core_data_retention.rs"]
mod core_data_retention;

#[cfg(test)]
#[path = "tests/core_retry.rs"]
mod core_retry;

#[cfg(test)]
#[path = "tests/core_mutation.rs"]
mod core_mutation;

#[cfg(test)]
#[path = "tests/core_infinite_query.rs"]
mod core_infinite_query;

#[cfg(test)]
#[path = "tests/core_select.rs"]
mod core_select;

#[cfg(test)]
#[path = "tests/core_network_mode.rs"]
mod core_network_mode;

// Integration tests (require GPUI test-support, available via dev-dep)
#[cfg(test)]
#[path = "tests/integration_client_fixtures.rs"]
mod integration_client_fixtures;

#[cfg(test)]
#[path = "tests/integration_client_bucket.rs"]
mod integration_client_bucket;

#[cfg(test)]
#[path = "tests/integration_client_client/mod.rs"]
mod integration_client_client;

#[cfg(test)]
#[path = "tests/integration_client_advanced/mod.rs"]
mod integration_client_advanced;
