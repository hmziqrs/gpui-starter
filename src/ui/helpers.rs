//! Shared UI helper re-exports.
//!
//! Aggregates the small, dependency-light styling utilities so feature pages
//! can pull them from a single location:
//!
//! ```ignore
//! use gpui_starter::ui::helpers::{lighten_color, darken_color};
//! ```
//!
//! These operate directly on [`gpui::Hsla`], so they compose cleanly with
//! colors pulled from `cx.theme()`.

pub use crate::ui::helpers_styled::{darken_color, lighten_color};
