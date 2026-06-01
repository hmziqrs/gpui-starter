//! Layer 0: Transport-agnostic query lifecycle primitives.
//!
//! `QueryResource` owns the cache/request state for one resource. Callers start
//! work with `begin_request`, then complete it with the returned `RequestId`.
//! Completion methods reject stale request ids, so cancelled or replaced async
//! work cannot overwrite newer state.
//!
//! This module depends only on `serde` — zero framework coupling.

mod error;
mod infinite_query;
mod key;
pub mod key_filter;
mod mutation;
mod network_mode;
mod policy;
mod refetch;
mod request;
mod resource;
mod retry;
mod select;
mod signal;
mod status;

pub use error::{QueryError, QueryErrorKind};
pub use infinite_query::InfiniteQueryResource;
pub use key::QueryKey;
pub use key_filter::QueryKeyFilter;
pub use mutation::{MutationResource, MutationStatus};
pub use network_mode::NetworkMode;
pub use policy::{CachePolicy, QueryBeginResult, QueryFetchMode, RequestPolicy};
pub use refetch::RefetchTrigger;
pub use request::{QueryTimestamp, RequestGuard, RequestId, RequestSequencer};
pub use resource::QueryResource;
pub use retry::RetryPolicy;
pub use select::{MappedQueryResource, SelectTransform};
pub use signal::QuerySignal;
pub use status::QueryStatus;
