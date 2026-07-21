//! Reusable, generic palette view.
//!
//! Composes a [`PaletteDelegate`] (via [`gpui_component::list::ListState`] +
//! [`gpui_component::list::List`]) with the gpui-starter
//! [`crate::ui::core::FocusManager`] helper. The concrete launcher in
//! [`crate::features::command_palette`] keeps its own bespoke rendering for now
//! (to preserve exact behaviour); this view is the generic, delegate-backed
//! replacement that future features can adopt directly.
//!
//! The view is generic over the entry type `E`; the `Kind` associated type is
//! inferred from [`PaletteEntry::Kind`].
//!
//! Note: this view uses only the verified public surface of `gpui` /
//! `gpui_component` (`ListState::new`, `List::new`, `InputState::new`,
//! `FocusManager`). The richer event-emission and search-wiring paths are
//! intentionally kept minimal here so the boilerplate compiles without
//! depending on less-stable `ListState` internals; adopters wire confirm /
//! query forwarding through their own controller (as the launcher adapter does).

use gpui::{FocusHandle, Focusable, IntoElement, Styled, prelude::*};
use gpui_component::{
    ActiveTheme as _, h_flex,
    input::{Input, InputState},
    list::{List, ListState},
    v_flex,
};

use crate::features::palette::delegate::PaletteDelegate;
use crate::features::palette::items::PaletteEntry;
use crate::ui::core::FocusManager;

const LOG: &str = "gpui_starter::palette::view";
const CONTEXT: &str = "Palette";

/// A generic, delegate-backed palette view.
///
/// Holds the [`ListState`] (which owns the [`PaletteDelegate`]) and a
/// [`FocusManager`]. The list's own internal search input drives
/// [`gpui_component::list::ListDelegate::perform_search`].
pub struct PaletteView<E: PaletteEntry + Clone + 'static> {
    focus: FocusManager,
    delegate_state: gpui::Entity<ListState<PaletteDelegate<E>>>,
    /// An optional external search input. When present it is rendered above the
    /// list; the list's internal input is still authoritative for filtering.
    input: Option<gpui::Entity<InputState>>,
}

impl<E: PaletteEntry + Clone + 'static> PaletteView<E> {
    /// Construct the view from an already-built delegate.
    ///
    /// The delegate is moved into a new [`ListState`]; `ListState::new` creates
    /// its own internal search input, so callers normally leave `input` as
    /// `None` and let the list own filtering. Pass `Some(input)` only to render
    /// a secondary, view-owned input.
    pub fn new(
        delegate: PaletteDelegate<E>,
        input: Option<gpui::Entity<InputState>>,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let focus = FocusManager::new(cx);
        let delegate_state = cx.new(|cx| ListState::new(delegate, window, cx));

        tracing::debug!(
            target: LOG,
            has_external_input = input.is_some(),
            "PaletteView constructed"
        );

        Self {
            focus,
            delegate_state,
            input,
        }
    }

    /// Borrow the underlying list state (and thus the delegate).
    pub fn state(&self) -> &gpui::Entity<ListState<PaletteDelegate<E>>> {
        &self.delegate_state
    }

    /// Borrow the optional external input, if one was supplied.
    pub fn input(&self) -> Option<&gpui::Entity<InputState>> {
        self.input.as_ref()
    }
}

impl<E: PaletteEntry + Clone + 'static> Focusable for PaletteView<E> {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.handle().clone()
    }
}

impl<E: PaletteEntry + Clone + 'static> gpui::Render for PaletteView<E> {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();

        let mut col = v_flex()
            .size_full()
            .bg(theme.background.opacity(0.0))
            .border_1()
            .border_color(theme.border.opacity(0.5))
            .rounded(theme.radius_lg)
            .key_context(CONTEXT)
            // Results list (delegate-driven). List renders its own search bar.
            .child(
                List::new(&self.delegate_state)
                    .flex_1()
                    .scrollbar_visible(true),
            )
            // Footer hint.
            .child(
                h_flex()
                    .px_4()
                    .py(gpui::px(8.))
                    .gap_4()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("↑↓  navigate")
                    .child("↵  open")
                    .child("esc  close"),
            );

        if let Some(input) = &self.input {
            col = col.child(
                Input::new(input)
                    .appearance(false)
                    .bordered(false)
                    .focus_bordered(false)
                    .flex_1(),
            );
        }

        col
    }
}
