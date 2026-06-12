//! Additional coverage tests for the QueryClient client layer (v2).
//!
//! Fills gaps not covered by `integration_client.rs`. Tests use `#[gpui::test]`
//! with `TestAppContext` and the `test_support` helpers.

mod client_basics;
mod client_gap_coverage;
mod client_mutations;
mod client_operations;
