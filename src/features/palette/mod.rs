//! Delegate / mode / view split for the command palette.
//!
//! This module refactors the historical monolithic launcher
//! ([`crate::features::command_palette`]) into composable, reusable pieces:
//!
//! - [`base_delegate::BaseDelegate<T>`] — generic selection-state helper
//!   (items, filtered indices, selection, query), independent of GPUI.
//! - [`items`] — the [`PaletteEntry`] trait model + [`EntryKind`] grouping key,
//!   the extension point for new entry kinds.
//! - [`filter::ItemFilter`] + [`filter::FuzzyMatchConfig`] — `SkimMatcherV2`
//!   fuzzy scoring with JSON-configurable bonuses / penalties.
//! - [`sections::SectionManager`] — groups filtered entries by kind.
//! - [`delegate::PaletteDelegate`] — a `gpui_component::list::ListDelegate`
//!   implementation wiring the above together.
//! - [`view::PaletteView`] — a generic, delegate-backed palette view.
//!
//! The concrete launcher adapter (`Launcher` / `LauncherRoot` / `open_launcher`)
//! still lives in [`crate::features::command_palette`]; it preserves the
//! existing public surface and delegates its internal item model to the types
//! defined here.
//!
//! All files are unconditionally compiled (no cfg gate); the module references
//! only gpui-starter existing modules, std/core, `gpui`, `gpui_component`,
//! `serde`, and the new `fuzzy-matcher` crate (declared in the integration
//! manifest — do NOT assume it is in `Cargo.toml` yet).

pub mod base_delegate;
pub mod delegate;
pub mod filter;
pub mod items;
pub mod sections;
pub mod view;

pub use base_delegate::BaseDelegate;
pub use delegate::{CancelCallback, ConfirmCallback, PaletteDelegate};
pub use filter::{FilteredItem, FuzzyMatchConfig, ItemFilter};
pub use items::{EntryKind, KindStr, PaletteEntry};
pub use sections::{GroupStats, SectionManager};
pub use view::PaletteView;
