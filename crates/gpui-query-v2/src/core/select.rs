//! Select/transform support for projecting query data into derived views.
//!
//! This module provides [`SelectTransform`] and [`MappedQueryResource`], which
//! together implement the "select" pattern found in TanStack Query: a query
//! resource holds raw data of type `T`, and a `MappedQueryResource` applies a
//! `SelectTransform<T, U>` to project it into type `U` without duplicating the
//! underlying cache entry.
//!
//! # Hook integration
//!
//! The [`use_query_select`] hook wires a `SelectTransform` into the `use_query`
//! lifecycle. It returns a `MappedQueryResource` entity that is kept in sync
//! with the underlying `QueryResource` via an observer. Each time the source
//! resource changes (fetch completes, cache hit, etc.), the mapped resource
//! re-reads the source data and the transform is applied on access.
//!
//! # Example
//!
//! ```
//! use gpui_query_v2::core::{SelectTransform, MappedQueryResource};
//!
//! // Raw query data: a list of users.
//! let users = vec!["Alice", "Bob", "Carol"];
//!
//! // Transform: extract just the count.
//! let transform = SelectTransform::new(|users: &Vec<&str>| users.len());
//!
//! let mapped = MappedQueryResource::<_, usize, ()>::new(Some(users), transform);
//! assert_eq!(mapped.data(), Some(3));
//! ```
//!
//! ## Example with the hook
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
//!         let (mapped, _, _subs) = use_query_select(
//!             QueryOptions::new("users"),
//!             count_transform,
//!             |signal| async move {
//!                 // Your async fetcher here
//!                 Ok(vec![])
//!             },
//!             cx,
//!         );
//!         Self { mapped, _subs }
//!     }
//! }
//! ```

use std::sync::Arc;

/// A select transform that maps cached data of type `T` to output type `U`.
///
/// Stored as `Arc<dyn Fn(&T) -> U>` to be `Clone + Send + Sync`. Use this
/// with [`MappedQueryResource`] to derive a projected view from cached query
/// data without storing a separate copy.
///
/// # Example
///
/// ```
/// use gpui_query_v2::core::SelectTransform;
///
/// let uppercase = SelectTransform::new(|name: &String| name.to_uppercase());
/// assert_eq!(uppercase.apply(&"hello".to_string()), "HELLO");
/// ```
pub struct SelectTransform<T, U> {
    transform: Arc<dyn Fn(&T) -> U + Send + Sync>,
    _marker: std::marker::PhantomData<(T, U)>,
}

impl<T, U> Clone for SelectTransform<T, U> {
    fn clone(&self) -> Self {
        Self {
            transform: Arc::clone(&self.transform),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T, U> std::fmt::Debug for SelectTransform<T, U> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectTransform").finish()
    }
}

impl<T, U> SelectTransform<T, U> {
    /// Create a new select transform from a closure.
    pub fn new(transform: impl Fn(&T) -> U + Send + Sync + 'static) -> Self {
        Self {
            transform: Arc::new(transform),
            _marker: std::marker::PhantomData,
        }
    }

    /// Apply the transform to data.
    pub fn apply(&self, data: &T) -> U {
        (self.transform)(data)
    }
}

/// A mapped view over a `QueryResource` that applies a [`SelectTransform`].
///
/// This implements the "select" pattern: multiple consumers can derive
/// different views from the same underlying cached data, each with their own
/// `MappedQueryResource` holding a different transform function. The source
/// data is shared, so there is no duplication.
///
/// # Type parameters
///
/// - `T`: The source data type (the cached query result).
/// - `U`: The projected output type (the derived view).
/// - `E`: The error type (carried through for API consistency).
#[derive(Clone, Debug)]
pub struct MappedQueryResource<T, U, E> {
    source_data: Option<T>,
    transform: SelectTransform<T, U>,
    _error_marker: std::marker::PhantomData<E>,
}

impl<T, U, E> MappedQueryResource<T, U, E> {
    /// Create a new mapped resource.
    pub fn new(source_data: Option<T>, transform: SelectTransform<T, U>) -> Self {
        Self {
            source_data,
            transform,
            _error_marker: std::marker::PhantomData,
        }
    }

    /// Apply the transform to get the selected data.
    ///
    /// **Note (audit #3):** This re-applies the transform closure on every call.
    /// `MappedQueryResource` is a derived view with no separate output cache — it
    /// stores only the source data and the transform function. If the transform is
    /// expensive and you need the result multiple times in a single render pass
    /// (e.g., once for display and once for an equality check), cache the result
    /// in a local variable:
    ///
    /// ```
    /// use gpui_query_v2::core::{MappedQueryResource, SelectTransform};
    ///
    /// let transform = SelectTransform::new(|v: &Vec<i32>| v.len());
    /// let mapped = MappedQueryResource::<_, usize, ()>::new(Some(vec![1, 2, 3]), transform);
    /// let data = mapped.data(); // transform runs once
    /// assert_eq!(data, Some(3));
    /// // use `data` freely below
    /// ```
    ///
    /// For lightweight transforms (field access, counting, simple projections) the
    /// cost is negligible and no caching is needed.
    pub fn data(&self) -> Option<U> {
        self.source_data.as_ref().map(|d| self.transform.apply(d))
    }

    /// Whether source data exists.
    pub fn has_data(&self) -> bool {
        self.source_data.is_some()
    }

    /// Read-only access to the source data.
    ///
    /// Used internally by the hook layer to compare source data by reference
    /// before cloning, avoiding unnecessary allocations when the data has not
    /// changed.
    pub fn source_data(&self) -> Option<&T> {
        self.source_data.as_ref()
    }

    /// Update the source data from the underlying query resource.
    ///
    /// Call this when the source `QueryResource` changes (fetch completes,
    /// cache invalidation, etc.) to keep the mapped view in sync. The transform
    /// is not applied here — it is applied lazily when [`data()`](Self::data)
    /// is called.
    pub fn update_source(&mut self, data: Option<T>) {
        self.source_data = data;
    }
}
