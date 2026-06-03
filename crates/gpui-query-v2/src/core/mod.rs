//! Layer 0: Transport-agnostic query lifecycle primitives.
//!
//! `QueryResource` owns the cache/request state for one resource. Callers start
//! work with `begin_request`, then complete it with the returned `RequestId`.
//! Completion methods reject stale request ids, so cancelled or replaced async
//! work cannot overwrite newer state.
//!
//! # Request lifecycle
//!
//! The typical lifecycle for a single query fetch is:
//!
//! 1. **Begin**: Call [`QueryResource::begin_request`] with a [`RequestSequencer`].
//!    This returns a [`QueryBeginResult`] indicating whether a fetch is needed,
//!    the cache was hit, or the request was ignored.
//!
//! 2. **Fetch**: If the result is [`Started`](QueryBeginResult::Started) or
//!    [`StaleCacheHit`](QueryBeginResult::StaleCacheHit), start an async fetch
//!    using the returned [`RequestId`].
//!
//! 3. **Accept**: When the fetch completes, call
//!    [`QueryResource::accept_current_request`] with the `RequestId`. If the
//!    request is still active (not replaced or cancelled), this returns a
//!    [`RequestGuard`] — a single-use capability token.
//!
//! 4. **Complete**: Pass the [`RequestGuard`] (by value) to
//!    [`QueryResource::complete_success`] or [`QueryResource::complete_failure`].
//!    The guard is consumed, preventing accidental double-completion.
//!
//! Alternatively, use the convenience methods [`QueryResource::complete_current_success`]
//! or [`QueryResource::complete_current_failure`] which combine steps 3 and 4.
//!
//! This module depends only on `serde` — zero framework coupling.

mod error;
mod infinite_query;
mod key;
pub mod key_filter;
mod mutation;
mod policy;
mod refetch;
mod request;
mod resource;
mod retry;
mod select;
mod signal;
mod status;

pub use error::{QueryError, QueryErrorKind};
pub use infinite_query::{FetchDirection, InfiniteQueryResource};
pub use key::QueryKey;
pub use key_filter::QueryKeyFilter;
pub use mutation::{MutationResource, MutationStatus};
pub use policy::{CachePolicy, QueryBeginResult, QueryFetchMode, RequestPolicy};
pub use refetch::RefetchTrigger;
pub use request::{QueryTimestamp, RequestGuard, RequestId, RequestSequencer};
pub use resource::QueryResource;
pub use retry::RetryPolicy;
pub use select::{MappedQueryResource, SelectTransform};
pub use signal::QuerySignal;
pub use status::QueryStatus;
