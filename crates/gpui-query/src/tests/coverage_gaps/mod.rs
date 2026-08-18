//! High-priority test coverage gaps for gpui-query.
//!
//! # Test categories
//!
//! 1. **Property-based tests** (no external framework): Systematic checks over
//!    many inputs for RetryPolicy, CachePolicy, serde roundtrip, RequestSequencer.
//!
//! 2. **State-transition invariant tests**: Table-driven verification that
//!    status and data are never inconsistent after any state transition.
//!
//! 3. **Deterministic GC eviction tests**: Concrete assertions on GC behavior
//!    rather than "no panic" patterns.
//!
//! 4. **Concurrency guard tests**: Verify that the two-phase completion protocol
//!    maintains invariants even when requests are interleaved.

mod property_based;
mod state_transitions;
mod gc_eviction;
mod concurrency;
mod gap_tests;
