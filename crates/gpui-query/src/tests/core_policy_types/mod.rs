//! Tests for RetryPolicy, RefetchTrigger, QueryError, QueryStatus,
//! QueryTimestamp, and RequestId edge cases.
//!
//! Covers untested scenarios across all core policy/value types.
//!
//! Note: NetworkMode is tested inline in core/network_mode.rs.

mod retry_policy;
mod query_error;
mod policy_and_status_types;
