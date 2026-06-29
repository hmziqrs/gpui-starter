//! Theme-correct section header atom.
//!
//! A small divider/label used to separate groups of items inside a list.
//! Colors are pulled from `cx.theme()` via [`gpui_component::ActiveTheme`];
//! there is no application-specific theme type.

use gpui::{App, Div, FontWeight, ParentElement as _, SharedString, Styled, div, px};
use gpui_component::ActiveTheme as _;

/// Build a section header (label + trailing divider) in one call.
///
/// `label` is upper-cased and rendered in the theme's muted foreground with a
/// semi-bold weight; a hairline divider fills the remaining width.
pub fn section_header(label: impl Into<SharedString>, cx: &App) -> Div {
    SectionHeader::new(label).render(cx)
}

/// Fluent builder for a section header.
///
/// Construct with [`SectionHeader::new`], optionally attach a count with
/// [`SectionHeader::count`], then call [`SectionHeader::render`].
pub struct SectionHeader {
    title: SharedString,
    count: Option<usize>,
}

impl SectionHeader {
    /// Create a new section header for the given label.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            count: None,
        }
    }

    /// Attach an item count, rendered as `"LABEL (N)"`.
    pub fn count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    /// Render the header. Colors come from `cx.theme()`.
    pub fn render(self, cx: &App) -> Div {
        let theme = cx.theme();

        let label: SharedString = match self.count {
            Some(n) => format!("{} ({})", uppercase(&self.title), n).into(),
            None => uppercase(&self.title).into(),
        };

        div()
            .px_3()
            .pt_3()
            .pb_1()
            .mx_1()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .h(px(1.))
                    .border_t_1()
                    .border_color(theme.border),
            )
    }
}

/// Uppercase an arbitrary `SharedString` without allocating when it is already
/// ASCII-uppercase. Falls back to a heap allocation only when needed.
fn uppercase(s: &SharedString) -> String {
    let owned: String = s.to_string();
    if owned.chars().all(|c| !c.is_ascii_lowercase()) {
        return owned;
    }
    owned.to_uppercase()
}
