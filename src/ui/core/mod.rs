//! Core UI building blocks: focus management and input handling.
//!
//! Reusable, view-agnostic helpers that wrap gpui / gpui_component primitives so
//! feature pages do not have to re-derive the same subscription boilerplate.
//! Rebinds to gpui-starter's own `gpui` / `gpui_component` types (see
//! `cx.theme()` via [`gpui_component::ActiveTheme`]) rather than any
//! application-specific theme type.

pub mod focus;
pub mod input_handler;

pub use focus::FocusManager;
pub use input_handler::InputHandler;
