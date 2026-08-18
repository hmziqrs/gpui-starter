//! The `use_infinite_query` hook — ergonomic infinite scrolling / pagination
//! for GPUI components.
//!
//! This module is split into three focused submodules:
//!
//! - [`hook`] — the main `use_infinite_query` hook function
//! - [`fetch_helpers`] — public `fetch_next_page_infinite` and `fetch_previous_page_infinite`
//! - [`fetch_runners`] — internal async fetch runners with retry logic

mod fetch_helpers;
mod fetch_runners;
mod hook;

pub use fetch_helpers::{fetch_next_page_infinite, fetch_previous_page_infinite};
pub use hook::use_infinite_query;
