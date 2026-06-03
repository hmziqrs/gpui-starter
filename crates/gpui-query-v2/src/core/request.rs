//! Request lifecycle primitives for the query system.
//!
//! This module provides the core types that govern how async requests are
//! identified, sequenced, and completed within the query framework:
//!
//! - [`RequestId`] — a unique, ordered identifier for each in-flight request.
//! - [`RequestSequencer`] — a monotonic generator of `RequestId` values, scoped
//!   per resource to guarantee uniqueness even after sequence overflow.
//! - [`RequestGuard`] — a single-use capability token that enforces the two-phase
//!   completion protocol (accept → complete).
//! - [`QueryTimestamp`] — a millisecond-precision timestamp used for cache
//!   freshness and staleness calculations.
//!
//! # Two-phase completion protocol
//!
//! The query system uses a two-phase protocol to safely complete async work:
//!
//! 1. **Accept**: Call [`QueryResource::accept_current_request`] with a
//!    [`RequestId`]. If the request is still active (not replaced or cancelled),
//!    this returns `Some(RequestGuard)`. Otherwise it returns `None`.
//!
//! 2. **Complete**: Pass the [`RequestGuard`] (by value) to one of the
//!    completion methods: [`QueryResource::complete_success`],
//!    [`QueryResource::complete_failure`],
//!    [`QueryResource::complete_success_optional`], or
//!    [`QueryResource::complete_failure_with_data`]. The guard is consumed,
//!    preventing double-completion.
//!
//! Convenience methods like [`QueryResource::complete_current_success`] combine
//! both phases into a single call.
//!
//! [`QueryResource`]: super::QueryResource

use serde::{Deserialize, Serialize};

/// A unique identifier for an in-flight request.
///
/// Combines a scope id (per-resource) with a monotonically increasing sequence.
/// Two `RequestId` values are equal only when both scope and sequence match.
/// Ordering is lexicographic: scope first, then sequence.
///
/// # Example
///
/// ```
/// use gpui_query_v2::core::RequestId;
///
/// let id = RequestId::scoped(1, 42);
/// assert_eq!(id.scope_id(), 1);
/// assert_eq!(id.value(), 42);
/// assert_eq!(id.label(), "1:42");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RequestId {
    scope_id: u64,
    sequence: u64,
}

impl RequestId {
    /// Create a request id with explicit scope and sequence.
    pub fn scoped(scope_id: u64, sequence: u64) -> Self {
        Self { scope_id, sequence }
    }

    /// The sequence number within this scope.
    pub fn value(self) -> u64 {
        self.sequence
    }

    /// The scope identifier.
    pub fn scope_id(self) -> u64 {
        self.scope_id
    }

    /// Human-readable label for diagnostics.
    pub fn label(self) -> String {
        format!("{}:{}", self.scope_id, self.sequence)
    }
}

/// Monotonic request id generator scoped to a single resource.
///
/// Each `RequestSequencer` produces a stream of [`RequestId`] values that are
/// unique within the resource's lifetime. The sequence counter increments
/// from 1; when it would overflow `u64::MAX`, the scope advances to avoid
/// producing duplicate ids.
///
/// # Scope advancement
///
/// When the sequence counter reaches `u64::MAX`, [`next_request`](Self::next_request)
/// calls [`advance_scope`](Self::advance_scope), which increments `scope_id`
/// and resets `next_request_id` to 1. This guarantees uniqueness across
/// the entire lifetime of the sequencer.
///
/// # Theoretical wrap-around
///
/// If `scope_id` itself overflows `u64::MAX`, it wraps back to 1 and
/// `next_request_id` is reset to 1. This means a new `RequestId(1, 1)` could
/// theoretically collide with a very old `RequestId(1, 1)` still held by a
/// long-running future. In practice, reaching `u64::MAX` requests per scope
/// is essentially impossible, so this is not a practical concern. For
/// extremely long-lived processes (e.g., a server running for decades), the
/// collision risk remains theoretical but documented here for completeness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestSequencer {
    pub(crate) scope_id: u64,
    pub(crate) next_request_id: u64,
}

impl Default for RequestSequencer {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestSequencer {
    /// Create a new sequencer starting at scope 1, sequence 1.
    pub fn new() -> Self {
        Self {
            scope_id: 1,
            next_request_id: 1,
        }
    }

    /// Generate the next request id.
    ///
    /// The sequence counter increments with each call. When it reaches
    /// `u64::MAX`, the scope advances automatically before the next call
    /// produces a duplicate.
    pub fn next_request(&mut self) -> RequestId {
        let request_id = RequestId::scoped(self.scope_id, self.next_request_id);
        if self.next_request_id == u64::MAX {
            self.advance_scope();
        } else {
            self.next_request_id += 1;
        }
        request_id
    }

    /// Advance to a new scope when the sequence overflows.
    ///
    /// Increments `scope_id` via checked addition. If `scope_id` itself
    /// overflows (astronomically unlikely), it wraps to 1 and the sequence
    /// resets, as documented on the struct.
    pub fn advance_scope(&mut self) {
        self.scope_id = self.scope_id.checked_add(1).unwrap_or(1);
        self.next_request_id = 1;
    }

    /// Whether the given request id belongs to the current scope.
    pub fn is_current_scope(&self, request_id: RequestId) -> bool {
        request_id.scope_id == self.scope_id
    }
}

/// A timestamp for query operations, in milliseconds since UNIX epoch.
///
/// Used for cache freshness checks (TTL, stale-while-revalidate) and for
/// recording when data was last updated. Obtain the current time via
/// `QueryTimestamp::from_millis(...)` using your application's clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct QueryTimestamp(u128);

impl QueryTimestamp {
    /// Create a timestamp from milliseconds.
    pub fn from_millis(value: u128) -> Self {
        Self(value)
    }

    /// The timestamp in milliseconds.
    pub fn as_millis(self) -> u128 {
        self.0
    }

    /// Compute elapsed time since an earlier timestamp.
    pub(super) fn elapsed_since(self, earlier: Self) -> Option<u128> {
        self.0.checked_sub(earlier.0)
    }
}

impl From<u128> for QueryTimestamp {
    fn from(value: u128) -> Self {
        Self::from_millis(value)
    }
}

/// A single-use capability token proving the holder owns the current request.
///
/// Created by [`QueryResource::accept_current_request`], consumed by one of the
/// `complete_*` methods. The guard is **moved** (not copied) into the
/// completion method, which enforces the two-phase protocol at the type level:
/// once a guard is used, it cannot be used again.
///
/// # Two-phase protocol
///
/// 1. **Accept**: `resource.accept_current_request(request_id)` validates that
///    the request is still active and returns `Some(RequestGuard)`.
/// 2. **Complete**: `resource.complete_success(guard, data, now_ms)` consumes
///    the guard and applies the result. Attempting to use the guard again is a
///    compile error because it has been moved.
///
/// # Why not `Copy`?
///
/// Previous versions derived `Clone` + `Copy`, which allowed the same guard to
/// be passed to multiple `complete_*` calls. While the second call would be a
/// no-op (the resource already cleared `active_request_id`), it was wasteful
/// and could mask bugs. Taking the guard by value prevents this entirely.
///
/// [`QueryResource`]: super::QueryResource
/// [`QueryResource::accept_current_request`]: super::QueryResource::accept_current_request
#[derive(Debug, PartialEq, Eq)]
pub struct RequestGuard {
    request_id: RequestId,
}

impl RequestGuard {
    pub(super) fn new(request_id: RequestId) -> Self {
        Self { request_id }
    }

    /// The request id this guard protects (borrowed).
    pub fn request_id(&self) -> RequestId {
        self.request_id
    }

    /// Consume the guard and return the request id.
    ///
    /// Useful when you want to extract the id and discard the guard.
    pub fn into_request_id(self) -> RequestId {
        self.request_id
    }
}
