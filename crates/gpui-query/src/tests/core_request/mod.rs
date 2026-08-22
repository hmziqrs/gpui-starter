//! Comprehensive tests for REQUEST MANAGEMENT in gpui-query.
//!
//! Covers:
//! - RequestId: construction, fields, ordering, label, equality
//! - RequestSequencer: monotonicity, scope advancement, wrapping at u64::MAX
//! - RequestGuard: proof-of-ownership, scope-based rejection, into_request_id
//! - RequestPolicy: LatestWins cancels previous, IgnoreWhileLoading keeps previous
//! - QuerySignal: creation per request, cancellation propagation across clones
//! - begin_request_with_id variant

mod request_id_sequencer;
mod request_lifecycle;
mod request_policy;
