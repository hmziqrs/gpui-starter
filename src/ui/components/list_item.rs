//! Theme-correct list row atom.
//!
//! A reusable list-row builder (`list_row`) plus a fluent `ListItem` struct,
//! both rendering an optional icon, an ellipsised title, an optional
//! description, a selected-state background pulled from `cx.theme()`, and an
//! optional action-key badge (e.g. `Enter`).
//!
//! Rebinds to gpui-starter's own types: colors come from the active
//! [`gpui_component::ActiveTheme`] (`cx.theme()`), and the [`Icon`] enum wraps
//! `gpui_component::{IconName, Icon}` and `gpui::Image`. There is no
//! application-specific theme type here.
//!
//! # Example
//! ```ignore
//! use gpui_starter::ui::components::{list_row, list_item::Icon};
//! use gpui_component::IconName;
//!
//! list_row(
//!     "list-row",
//!     "Open file",
//!     Some(&Icon::Named(IconName::File)),
//!     Some("Open the selected document"),
//!     true,
//!     Some("Enter"),
//!     &cx,
//! )
//! ```

use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Div, ElementId, FontWeight, Hsla, Image, ImageFormat, ImageSource,
    InteractiveElement as _, ParentElement as _, SharedString, Stateful, Styled, div, img, px,
    relative,
};
use gpui_component::{ActiveTheme as _, Icon as ComponentIcon, IconName, Sizable as _};

const LOG: &str = "gpui_starter::ui::components::list_item";

/// A generalized icon source for a list row.
///
/// Rebinds to gpui-starter's icon types so callers never touch a
/// launcher-specific enum. Each variant maps to a concrete rendering strategy
/// inside [`Icon::render`]:
///
/// - [`Icon::Named`]   -> a `gpui_component::IconName` (SVG from the asset bundle)
/// - [`Icon::Path`]    -> an asset-bundle path string, e.g. `"icons/foo.svg"`
/// - [`Icon::Data`]    -> an in-memory raster `gpui::Image` (PNG/JPEG bytes)
/// - [`Icon::Placeholder`] -> a short text glyph shown in a muted badge
#[derive(Clone)]
pub enum Icon {
    /// A named Phosphor icon from `gpui_component`'s asset bundle.
    Named(IconName),
    /// A path into `gpui_component`'s asset bundle, e.g. `"icons/foo.svg"`.
    Path(SharedString),
    /// An in-memory raster image (e.g. decoded PNG/JPEG bytes).
    Data(Arc<Image>),
    /// A short placeholder string rendered inside a muted badge.
    Placeholder(SharedString),
}

impl Icon {
    /// Build an [`Icon::Data`] from raw image bytes (e.g. downloaded PNG).
    ///
    /// Falls back to [`Icon::Placeholder`] if the bytes cannot be wrapped in a
    /// `gpui::Image`; we never panic on bad icon data — we degrade gracefully
    /// and log via the `LOG` target.
    pub fn from_image_bytes(format: ImageFormat, bytes: Vec<u8>) -> Self {
        if bytes.is_empty() {
            tracing::warn!(target: LOG, "from_image_bytes: empty byte buffer");
            return Self::Placeholder("?".into());
        }
        Self::Data(Arc::new(Image::from_bytes(format, bytes)))
    }

    /// Render this icon into an element suitable as a list-row child.
    ///
    /// `bg` is the badge background (typically `cx.theme().secondary`) and
    /// `fg` the badge foreground (`cx.theme().muted_foreground`).
    pub fn render(&self, bg: Hsla, fg: Hsla, radius: gpui::Pixels) -> Div {
        let badge = div()
            .flex()
            .flex_shrink_0()
            .size_8()
            .items_center()
            .justify_center()
            .rounded(radius)
            .bg(bg);

        match self {
            Icon::Named(name) => {
                badge.child(ComponentIcon::new(name.clone()).small().text_color(fg))
            }
            Icon::Path(path) => {
                badge.child(ComponentIcon::default().path(path.clone()).text_color(fg))
            }
            Icon::Data(image) => badge.child(
                img(ImageSource::Image(image.clone()))
                    .w(px(20.))
                    .h(px(20.))
                    .rounded(radius),
            ),
            Icon::Placeholder(text) => badge.text_xs().text_color(fg).child(text.clone()),
        }
    }
}

impl From<IconName> for Icon {
    fn from(name: IconName) -> Self {
        Self::Named(name)
    }
}

/// The concrete row returned by [`list_row`] / [`ListItem::render`].
pub type Row = Stateful<Div>;

/// Build a complete list row in one call.
///
/// This is the flat convenience form of the [`ListItem`] builder. All colors are
/// pulled from `cx.theme()`.
///
/// - `id`          stable element id (required for interactivity / statefulness)
/// - `title`       primary label, ellipsised on overflow
/// - `icon`        optional [`Icon`]
/// - `description` optional secondary line, ellipsised on overflow
/// - `selected`    toggles the selected-state background (`theme.list_active`)
/// - `action_key`  optional right-aligned action badge label (e.g. `"Enter"`)
#[allow(clippy::too_many_arguments)]
pub fn list_row(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    icon: Option<&Icon>,
    description: Option<&str>,
    selected: bool,
    action_key: Option<&str>,
    cx: &App,
) -> Row {
    let mut item = ListItem::new(id, selected).title(title);
    if let Some(ic) = icon {
        item = item.icon(ic.clone());
    }
    if let Some(desc) = description {
        item = item.description(desc);
    }
    if let Some(key) = action_key {
        item = item.action_key(key);
    }
    item.render(cx)
}

/// Fluent builder for a theme-correct list row.
///
/// Construct with [`ListItem::new`], chain the `.icon/.title/.description/
/// .action_key` setters, then call [`ListItem::render`] (or build via
/// [`list_row`] for the one-shot form).
pub struct ListItem {
    id: ElementId,
    selected: bool,
    icon: Option<Icon>,
    title: SharedString,
    description: Option<SharedString>,
    action_key: Option<SharedString>,
}

impl ListItem {
    /// Create a new row builder. `selected` controls the background.
    pub fn new(id: impl Into<ElementId>, selected: bool) -> Self {
        Self {
            id: id.into(),
            selected,
            icon: None,
            title: "".into(),
            description: None,
            action_key: None,
        }
    }

    /// Set the primary title (ellipsised on overflow).
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = title.into();
        self
    }

    /// Set an icon for the row.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Set an optional secondary description line.
    pub fn description(mut self, desc: impl Into<SharedString>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set an optional right-aligned action-key badge label (e.g. `"Enter"`).
    ///
    /// The badge is only shown when the row is selected.
    pub fn action_key(mut self, key: impl Into<SharedString>) -> Self {
        self.action_key = Some(key.into());
        self
    }

    /// Render the row. Colors and radii come from `cx.theme()`.
    pub fn render(self, cx: &App) -> Row {
        let theme = cx.theme();

        let bg = if self.selected {
            theme.list_active
        } else {
            theme.list_hover
        };

        let mut row = div()
            .id(self.id)
            .px_3()
            .py_2()
            .mx_1()
            .gap_3()
            .items_center()
            .rounded(theme.radius)
            .bg(bg)
            .relative()
            .flex()
            .flex_row()
            // Keep non-selected rows hoverable; selected rows keep their fill.
            .when(!self.selected, |el| el.hover(|el| el.bg(theme.list_active)));

        if let Some(ic) = self.icon {
            row = row.child(ic.render(theme.secondary, theme.muted_foreground, theme.radius));
        }

        // Text column: title (always) + optional description, both ellipsised.
        let mut text_col = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.foreground)
                    .truncate()
                    .child(self.title),
            );

        if let Some(desc) = self.description {
            text_col = text_col.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .truncate()
                    .child(desc),
            );
        }
        row = row.child(text_col);

        // Right-aligned action-key badge, only when selected and provided.
        if self.selected {
            if let Some(key) = self.action_key {
                row = row.child(render_action_key(
                    &key,
                    theme.border,
                    theme.muted_foreground,
                ));
            }
        }

        row
    }
}

/// Render the small right-aligned action-key badge (e.g. the `↵ Enter` hint).
fn render_action_key(label: &SharedString, border: Hsla, fg: Hsla) -> Div {
    div()
        .absolute()
        .right_3()
        .top_0()
        .bottom_0()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(div().text_xs().text_color(fg).child(label.clone()))
        .child(
            div()
                .px_1()
                .py_0()
                .border_1()
                .border_color(border)
                .rounded(px(3.))
                .text_xs()
                .line_height(relative(1.))
                .text_color(fg)
                .child(SharedString::from("↵")),
        )
}
