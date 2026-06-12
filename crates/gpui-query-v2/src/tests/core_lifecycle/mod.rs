//! Comprehensive tests for the core lifecycle of QueryResource (v2).
//!
//! Covers all state transitions, cancellation, stale request rejection,
//! reset, retry counter management, signal lifecycle, and request policies.

mod cancel_and_signals;
mod data_and_lifecycle;
mod policies_and_cache;
mod reset_and_retry;
mod transitions;
