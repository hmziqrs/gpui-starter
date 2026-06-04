//! The `use_query_select` hook — combines `use_query` with a [`SelectTransform`].
//!
//! TanStack Query's `select` option transforms cached data into a derived shape,
//! re-running only when data changes. This module provides the same pattern for
//! gpui-query-v2: it wraps a [`QueryResource`] with a [`MappedQueryResource`]
//! entity that applies the transform on each observer notification.
//!
//! # Why a separate hook?
//!
//! Rust's type system requires knowing `T` (source) and `U` (output) at compile
//! time. Since `QueryOptions` is not generic over a transform output type, the
//! `select` field cannot live on `QueryOptions` without making the entire options
//! struct generic. Instead, [`use_query_select`] is a standalone hook that accepts
//! the transform as a separate parameter and returns a
//! `MappedQueryResource<T, U, E>` entity.
//!
//! # Usage
//!
//! ```no_run
//! use gpui_query_v2::hook::{use_query_select, QueryOptions};
//! use gpui_query_v2::core::SelectTransform;
//! # #[derive(Clone, PartialEq)]
//! # struct User;
//! # #[derive(Clone, Debug)]
//! # struct MyError;
//!
//! struct UserCountView {
//!     mapped: gpui::Entity<gpui_query_v2::core::MappedQueryResource<Vec<User>, usize, MyError>>,
//!     _subs: (gpui::Subscription, gpui::Subscription),
//! }
//!
//! impl UserCountView {
//!     fn new(cx: &mut gpui::Context<Self>) -> Self {
//!         let count_transform = SelectTransform::new(|users: &Vec<User>| users.len());
//!         let (mapped, query_entity, subs) = use_query_select(
//!             QueryOptions::new("users"),
//!             count_transform,
//!             |signal| async move {
//!                 // Your async fetcher here
//!                 Ok(vec![])
//!             },
//!             cx,
//!         );
//!         Self { mapped, _subs: subs }
//!     }
//! }
//! ```

use gpui::{AppContext as _, Context, Entity, Subscription};

use crate::core::{MappedQueryResource, QueryResource, SelectTransform};

use super::{use_query, QueryOptions};

/// Subscribe to a query and project its data through a [`SelectTransform`].
///
/// This is the "select" integration point for the hook layer (audit #3, HIGH
/// finding). It:
///
/// 1. Calls [`use_query`] to create/subscribe to the underlying `QueryResource`.
/// 2. Creates a `MappedQueryResource<T, U, E>` entity seeded with the current
///    source data.
/// 3. Observes the source entity so that every time it changes, the mapped
///    resource's source data is updated from the fresh `QueryResource::data()`.
///    The transform itself is applied lazily when
///    [`MappedQueryResource::data()`] is called.
///
/// # Returns
///
/// A tuple of:
/// - `Entity<MappedQueryResource<T, U, E>>` — the projected view entity
/// - `Entity<QueryResource<T, E>>` — the underlying query entity (for status,
///   error, refetch, etc.)
/// - `(Subscription, Subscription)` — the query subscription and the mapped
///   observer subscription. Store both to keep observations alive.
///
/// # Transform cost
///
/// The transform closure runs every time `mapped.data()` is called (no output
/// cache). For expensive transforms, cache the result:
///
/// ```no_run
/// use gpui_query_v2::core::{MappedQueryResource, SelectTransform};
/// # fn _doc(mapped: &gpui::Entity<MappedQueryResource<Vec<String>, usize, ()>>, cx: &gpui::App) {
///
/// let count = mapped.read(cx).data(); // transform runs once
/// // reuse `count` below
/// # }
/// ```
pub fn use_query_select<T, U, E, C, F, Fut>(
    options: impl Into<QueryOptions>,
    transform: SelectTransform<T, U>,
    fetcher: F,
    cx: &mut Context<C>,
) -> (
    Entity<MappedQueryResource<T, U, E>>,
    Entity<QueryResource<T, E>>,
    (Subscription, Subscription),
)
where
    T: Clone + PartialEq + Send + Sync + 'static,
    U: 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    F: Fn(crate::core::QuerySignal) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    // Step 1: Create the underlying query entity and start the fetch.
    let (query_entity, query_subscription) = use_query(options, fetcher, cx);

    // Step 2: Seed the mapped resource with whatever data the query has now.
    let initial_data: Option<T> =
        query_entity.read_with(cx, |r, _| r.data().cloned());
    let mapped = MappedQueryResource::new(initial_data, transform);
    let mapped_entity = cx.new(|_| mapped);

    // Step 3: Observe the query entity so the mapped resource stays in sync.
    // Every time the query entity is updated (fetch completes, refetch, cache
    // invalidation, etc.), we compare the new data against the cached source
    // data by reference before cloning. This avoids cloning the entire source
    // data T on every observer notification when nothing has changed.
    let mapped_weak = mapped_entity.downgrade();
    let mapped_subscription = cx.observe(&query_entity, move |_, entity, cx| {
        if let Some(mapped) = mapped_weak.upgrade() {
            let changed = mapped.read_with(cx, |m, _| {
                let fresh_ref = entity.read(cx).data();
                match (m.source_data(), fresh_ref) {
                    (Some(cached), Some(fresh)) => cached != fresh,
                    (None, None) => false,
                    _ => true,
                }
            });
            if changed {
                let fresh_data: Option<T> = entity.read(cx).data().cloned();
                mapped.update(cx, |m, _| {
                    m.update_source(fresh_data);
                });
            }
        }
    });

    (
        mapped_entity,
        query_entity,
        (query_subscription, mapped_subscription),
    )
}
