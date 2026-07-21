//! Reusable markdown rendering helper.
//!
//! Builds on gpui-component's [`TextView::markdown`] primitive (the same one
//! gpui-starter already pulls in via `gpui_component::text`) to render full
//! GFM markdown into an [`AnyElement`]:
//!
//! - Paragraphs and headings (H1-H6) with a level-based size mapping
//! - Bold, italic, strikethrough, inline code
//! - Fenced code blocks with syntax highlighting
//! - Links, images, ordered/unordered lists (nested), blockquotes, tables, rules
//!
//! Theme binding is generic boilerplate: the code-block background and corner
//! radius come from `cx.theme()` (see [`gpui_component::ActiveTheme`]), and the
//! dark/light [`HighlightTheme`] is selected from the window's
//! [`gpui::WindowAppearance`] (falling back to the active theme mode when the
//! platform does not report a luminance).

use std::sync::Arc;

use gpui::{
    AnyElement, App, ElementId, IntoElement, SharedString, StyleRefinement, Window,
    WindowAppearance, div, prelude::*, px, rems,
};
use gpui_component::ActiveTheme as _;
use gpui_component::highlighter::HighlightTheme;
use gpui_component::text::{TextView, TextViewStyle};

/// Render markdown into an [`AnyElement`] using a fixed element id.
///
/// This is the canonical entry point for feature pages that need to display
/// rich text (chat/agent responses, rendered notes, about-style content). The
/// `id` is forwarded to [`TextView::markdown`] as its [`ElementId`] so selection
/// state stays stable across re-renders; pass a unique per-call value when
/// embedding more than one markdown block on the same screen.
///
/// Dark vs. light syntax highlighting is chosen from the window appearance
/// (dark when the platform reports a dark background, matching the
/// `luminance < 0.5` intent), with the active theme mode as a fallback. Code
/// blocks are themed via `cx.theme()` — `muted` background, `radius` corners.
pub fn render_markdown_with_id(
    id: impl Into<ElementId>,
    markdown: &str,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme();

    // The window's OS-reported appearance is the most faithful signal for
    // "is the background dark" (luminance < 0.5). WindowAppearance exposes no
    // numeric luminance, so we map its dark variants directly and fall back to
    // the theme's configured mode for platforms that report only Light.
    let is_dark = match window.appearance() {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => true,
        WindowAppearance::Light | WindowAppearance::VibrantLight => theme.mode.is_dark(),
    };

    let highlight_theme = if is_dark {
        HighlightTheme::default_dark()
    } else {
        HighlightTheme::default_light()
    };

    let code_block_bg = theme.muted;
    let code_block_radius = theme.radius;

    let style = TextViewStyle {
        paragraph_gap: rems(1.5),
        heading_base_font_size: px(14.0),
        heading_font_size: Some(Arc::new(|level, _base| match level {
            1 => px(16.0),
            2 => px(14.0),
            3 => px(13.0),
            _ => px(12.0),
        })),
        highlight_theme,
        code_block: StyleRefinement::default()
            .bg(code_block_bg)
            .rounded(code_block_radius),
        // `table` / `table_cell` were added to `TextViewStyle` in gpui-component
        // rev e416af7 (v0.5.2); default both — no table-specific styling needed.
        table: StyleRefinement::default(),
        table_cell: StyleRefinement::default(),
        is_dark,
    };

    let id: ElementId = id.into();
    let text: SharedString = markdown.to_string().into();

    // Wrap in a text_sm container so the rendered markdown matches the rest of
    // the UI's default body sizing.
    div()
        .text_sm()
        .child(TextView::markdown(id, text).style(style).selectable(true))
        .into_any_element()
}

/// Render markdown with a default element id.
///
/// Convenience wrapper around [`render_markdown_with_id`] for the common case
/// of a single markdown block per view. Prefer the `_with_id` variant when
/// embedding multiple blocks to avoid element-id collisions.
pub fn render_markdown(markdown: &str, window: &mut Window, cx: &mut App) -> AnyElement {
    render_markdown_with_id("markdown", markdown, window, cx)
}
