//! The `use_query` and `use_mutation` hooks — ergonomic query and mutation
//! subscriptions for GPUI components.
//!
//! # v2 Improvements
//!
//! - Uses `QueryObserver` which returns `Option<Subscription>` instead of panicking
//! - Signals are properly cancelled on `LatestWins` replacement and `reset()`
//! - `AHashMap` in `QueryClient` for faster lookups
//! - `MutationDiagnostic` is a real type in devtools
//! - `max_pages` defaults to `Some(50)`
//! - `QueryError` has full `Display` + `Error` impls
//!
//! # Query Usage (options-first)
//!
//! The primary API is **options-first** with sensible defaults. The fetcher
//! always receives a [`QuerySignal`] for cooperative cancellation:
//!
//! ```no_run
//! use gpui_query_v2::hook::use_query;
//! use gpui_query_v2::{QueryOptions, CachePolicy, RequestPolicy};
//! # #[derive(Clone)]
//! # struct User;
//! # #[derive(Clone, Debug)]
//! # struct MyError;
//!
//! struct MyView {
//!     users: gpui::Entity<gpui_query_v2::QueryResource<Vec<User>, MyError>>,
//!     _subscription: gpui::Subscription,
//! }
//!
//! impl MyView {
//!     fn new(cx: &mut gpui::Context<Self>) -> Self {
//!         let (users, _subscription) = use_query(
//!             QueryOptions::new("users")
//!                 .cache_policy(CachePolicy::Ttl { ttl_ms: 60_000 })
//!                 .request_policy(RequestPolicy::LatestWins),
//!             |signal| async move {
//!                 // Your async fetcher here
//!                 Ok(vec![])
//!             },
//!             cx,
//!         );
//!         Self { users, _subscription }
//!     }
//! }
//! ```
//!
//! For backward compatibility, [`use_query_unsignalled`] is available with a
//! `Fn() -> Fut` fetcher that receives no signal. However, the signal-accepting
//! `use_query` is the recommended default per the v2 "Signal-always" design goal.
//!
//! # Mutation Usage
//!
//! ```no_run
//! use gpui_query_v2::hook::{use_mutation, mutate};
//! # #[derive(Clone)]
//! # struct NewUser { name: String }
//! # #[derive(Clone)]
//! # struct User;
//! # #[derive(Clone, Debug)]
//! # struct MyError;
//!
//! struct MyView {
//!     create_user: gpui::Entity<gpui_query_v2::MutationResource<NewUser, User, MyError>>,
//!     _subscription: gpui::Subscription,
//! }
//!
//! impl MyView {
//!     fn new(cx: &mut gpui::Context<Self>) -> Self {
//!         let (entity, sub) = use_mutation((), cx);
//!         Self { create_user: entity, _subscription: sub }
//!     }
//!
//!     fn handle_submit(&mut self, name: String, cx: &mut gpui::Context<Self>) {
//!         mutate(&self.create_user, NewUser { name }, |vars| async move {
//!             Ok(User)
//!         }, cx);
//!     }
//! }
//! ```
//!
//! # WeakEntity Discard Behavior
//!
//! Throughout this module, [`gpui::WeakEntity::upgrade()`] is used inside async
//! tasks to access the owning entity. If the owning component is unmounted while
//! a fetch is in-flight, `upgrade()` returns `None` and the fetch result is
//! **silently discarded**. This is intentional for cache-layer correctness (avoids
//! writing to a dead entity), but callers who rely on side effects from fetch
//! completion should be aware that no callback or notification fires in this case.
//!
//! # Signal Cancellation (Audit Finding #8)
//!
//! The `accept_current_request` guard is the authoritative protection against stale
//! writes. A previous `signal.is_cancelled()` check after the fetcher returned was
//! removed -- it was a best-effort optimization with a TOCTOU window that provided
//! no guarantees. The two-phase protocol (accept + complete) correctly handles all
//! cases where a newer request supersedes the current one.

mod fetch_retry;
mod mutation_hooks;
mod options;
mod query_hooks;
mod use_infinite_query;
mod use_query_select;

// ── Re-exports from options ─────────────────────────────────────────────

pub use options::{InfiniteQueryOptions, MutationCallbacks, MutationOptions, QueryOptions};

// ── Re-exports from query_hooks ─────────────────────────────────────────

pub use query_hooks::{
    fetch_query, fetch_query_with_signal, use_query, use_query_manual, use_query_unsignalled,
};

// ── Re-exports from use_infinite_query ───────────────────────────────────

pub use use_infinite_query::{
    fetch_next_page_infinite, fetch_previous_page_infinite, use_infinite_query,
};

// ── Re-exports from use_query_select ─────────────────────────────────────

pub use use_query_select::use_query_select;

// ── Re-exports from mutation_hooks ───────────────────────────────────────

pub use mutation_hooks::{
    mutate, mutate_with_callbacks, use_mutation, use_mutation_state, use_mutation_with_options,
};

// ── Utility ─────────────────────────────────────────────────────────────

/// Returns current time as milliseconds since UNIX epoch.
///
/// Audit fix #20: This is the canonical implementation used across the hook
/// layer. The private duplicate in `mutation_bucket.rs` (`now_ms`) should
/// ideally be consolidated here or into a shared utility module.
pub fn current_time_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

// ── Impl for MutationOptions integration ────────────────────────────────

/// Allow `use_mutation((), cx)` to work with default options.
impl From<()> for MutationOptions {
    fn from((): ()) -> Self {
        Self::default()
    }
}
