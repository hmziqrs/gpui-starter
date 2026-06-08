//! The `use_query` and `use_mutation` hooks — ergonomic query and mutation
//! subscriptions for GPUI components.
//!
//! # Query Usage
//!
//! ```ignore
//! use gpui_query::hook::use_query;
//! use gpui_query::{CachePolicy, QueryKey, RequestPolicy};
//!
//! struct MyView {
//!     users: gpui::Entity<gpui_query::QueryResource<Vec<User>>>,
//!     _subscription: gpui::Subscription,
//! }
//!
//! impl MyView {
//!     fn new(cx: &mut gpui::Context<Self>) -> Self {
//!         let (users, _subscription) = use_query(
//!             QueryKey::from(["users"]),
//!             CachePolicy::Ttl { ttl_ms: 60_000 },
//!             RequestPolicy::LatestWins,
//!             || async {
//!                 let resp = reqwest::get("/api/users").await?;
//!                 let users: Vec<User> = resp.json().await?;
//!                 Ok(users)
//!             },
//!             cx,
//!         );
//!         Self { users, _subscription }
//!     }
//! }
//! ```
//!
//! # Mutation Usage
//!
//! ```ignore
//! use gpui_query::hook::{use_mutation, mutate};
//!
//! struct MyView {
//!     create_user: gpui::Entity<gpui_query::MutationResource<NewUser, User>>,
//! }
//!
//! impl MyView {
//!     fn new(cx: &mut gpui::Context<Self>) -> Self {
//!         let entity = use_mutation(cx);
//!         Self { create_user: entity }
//!     }
//!
//!     fn handle_submit(&mut self, name: String, cx: &mut gpui::Context<Self>) {
//!         mutate(&self.create_user, NewUser { name }, |vars| async move {
//!             api::create_user(&vars).await
//!         }, cx);
//!     }
//! }
//! ```

mod helpers;
mod options;
mod use_infinite_query;
mod use_mutation;
mod use_query;

pub use options::{MutationCallbacks, MutationOptions, QueryOptions};
pub use use_infinite_query::{
    fetch_next_page_infinite, fetch_previous_page_infinite, InfiniteQueryOptions,
    use_infinite_query,
};
pub use use_mutation::{
    mutate, mutate_with_callbacks, use_mutation, use_mutation_state, use_mutation_with_options,
};
pub use use_query::{
    fetch_query, fetch_query_with_signal, use_query, use_query_manual, use_query_with_signal,
};

// Shared utility re-exported for internal use by sibling modules.
pub(crate) use helpers::current_time_ms;
