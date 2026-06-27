//! Reusable, theme-correct UI atoms.
//!
//! Small, view-agnostic building blocks (list rows, section dividers) that every
//! feature page can compose without re-deriving the same `cx.theme()` styling.
//! These atoms rebind to gpui-starter's own `gpui` / `gpui_component` types —
//! colors come straight from the active theme via
//! [`gpui_component::ActiveTheme`] (`cx.theme()`), so there is no
//! application-specific theme type to keep in sync.

pub mod list_item;
pub mod section_header;

pub use list_item::{Icon as ListItemIcon, ListItem, list_row};
pub use section_header::{SectionHeader, section_header};
