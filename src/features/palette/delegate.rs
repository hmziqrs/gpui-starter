//! `PaletteDelegate`: a [`gpui_component::list::ListDelegate`] implementation
//! backed by [`BaseDelegate`], [`ItemFilter`], and [`SectionManager`].
//!
//! This is the reusable, kind-generic delegate that turns any slice of items
//! implementing [`PaletteEntry`] into a sectioned, fuzzy-searchable list usable
//! by [`gpui_component::list::List`]. The concrete launcher adapter lives in
//! [`crate::features::command_palette`].
//!
//! Ported from the reference launcher's `ItemListDelegate` but stripped of launcher-specific
//! concerns (dynamic calculator/AI items, best-match promotion, config modules)
//! and rebound to gpui-starter's generic entry model.

use std::sync::Arc;

use gpui::{Context, Task, Window, prelude::*};
use gpui_component::{
    ActiveTheme as _, IndexPath,
    list::{ListDelegate, ListState},
};

use crate::features::palette::base_delegate::BaseDelegate;
use crate::features::palette::filter::{FuzzyMatchConfig, ItemFilter};
use crate::features::palette::items::PaletteEntry;
use crate::features::palette::sections::SectionManager;

const LOG: &str = "gpui_starter::palette::delegate";

/// Callback invoked when the user confirms (Enter / click) an entry.
pub type ConfirmCallback<E> = Arc<dyn Fn(&E) + Send + Sync>;

/// Callback invoked when the palette is cancelled (Escape).
pub type CancelCallback = Arc<dyn Fn() + Send + Sync>;

/// A sectioned, fuzzy-searchable list delegate.
///
/// Generic over the concrete entry type `E` (must implement [`PaletteEntry`]);
/// the entry's [`PaletteEntry::Kind`] determines grouping. The delegate owns:
///
/// - a [`BaseDelegate`] for selection state,
/// - an [`ItemFilter`] for scoring,
/// - a [`SectionManager`] for grouping,
/// - optional confirm/cancel callbacks.
///
/// Selection and query are driven by the `ListDelegate` methods
/// [`ListDelegate::perform_search`], [`ListDelegate::set_selected_index`],
/// [`ListDelegate::confirm`] and [`ListDelegate::cancel`].
pub struct PaletteDelegate<E: PaletteEntry + Clone + 'static> {
    base: BaseDelegate<E>,
    filter: ItemFilter,
    sections: SectionManager<E::Kind>,
    on_confirm: Option<ConfirmCallback<E>>,
    on_cancel: Option<CancelCallback>,
}

impl<E: PaletteEntry + Clone + 'static> PaletteDelegate<E> {
    /// Construct a delegate over the given items using default scoring config
    /// and an empty preferred-group order (pure first-seen grouping).
    pub fn new(items: Vec<E>) -> Self {
        Self::with_config(items, FuzzyMatchConfig::default(), Vec::new())
    }

    /// Construct with explicit scoring configuration and a preferred group
    /// order (kinds listed first appear first when present).
    pub fn with_config(items: Vec<E>, config: FuzzyMatchConfig, preferred: Vec<E::Kind>) -> Self {
        let mut sections = SectionManager::new(preferred);
        // Seed section stats from the full, unfiltered set.
        let all: Vec<usize> = (0..items.len()).collect();
        sections.rebuild(&items, &all);
        Self {
            base: BaseDelegate::new(items),
            filter: ItemFilter::new(config),
            sections,
            on_confirm: None,
            on_cancel: None,
        }
    }

    /// Register the confirm callback (replaces any prior one).
    pub fn set_on_confirm(&mut self, callback: impl Fn(&E) + Send + Sync + 'static) {
        self.on_confirm = Some(Arc::new(callback));
    }

    /// Register the cancel callback (replaces any prior one).
    pub fn set_on_cancel(&mut self, callback: impl Fn() + Send + Sync + 'static) {
        self.on_cancel = Some(Arc::new(callback));
    }

    /// Borrow the underlying base delegate.
    pub fn base(&self) -> &BaseDelegate<E> {
        &self.base
    }

    /// Borrow the underlying section manager.
    pub fn sections(&self) -> &SectionManager<E::Kind> {
        &self.sections
    }

    /// Re-score and re-group against the stored query, then refresh selection.
    fn refilter(&mut self) {
        let query = self.base.query().to_string();
        let scored = self.filter.filter_with_scores(self.base.items(), &query);
        let indices: Vec<usize> = scored.iter().map(|f| f.index).collect();
        self.base.apply_filtered_indices(indices.clone());
        self.sections.rebuild(self.base.items(), &indices);

        if self.base.selected_index().is_none() && self.base.filtered_count() > 0 {
            self.base.set_selected_unchecked(0);
        }
    }

    /// Resolve a flat filtered position to its underlying entry, if any.
    fn entry_at(&self, row: usize) -> Option<&E> {
        self.base.get_filtered_item(row)
    }
}

impl<E: PaletteEntry + Clone + 'static> ListDelegate for PaletteDelegate<E> {
    // gpui_component's own list item: `Selectable + IntoElement`.
    type Item = gpui_component::list::ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.base.set_query(query.to_string());
        self.refilter();
        tracing::debug!(
            target: LOG,
            query = %query,
            results = self.base.filtered_count(),
            "palette search performed"
        );
        Task::ready(())
    }

    fn sections_count(&self, _cx: &gpui::App) -> usize {
        // At least 1 so the list always renders; empty groups are pruned by the
        // component when items_count returns 0.
        self.sections.group_count().max(1)
    }

    fn items_count(&self, section: usize, _cx: &gpui::App) -> usize {
        // We expose a single logical group: the flat filtered list. Multi-group
        // rendering is delegated to a richer subclass; the boilerplate keeps a
        // single section so behaviour matches the original monolithic launcher.
        if section == 0 {
            self.base.filtered_count()
        } else {
            0
        }
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let row = ix.row;
        let entry = self.entry_at(row)?;
        let selected = self.base.selected_index() == Some(row);

        let mut item = gpui_component::list::ListItem::new(("palette-row", row))
            .selected(selected)
            .px_3()
            .py_2()
            .mx_1()
            .gap_3()
            .items_center()
            .rounded(cx.theme().radius)
            .child(
                gpui::div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(
                        gpui::div()
                            .text_sm()
                            .child(gpui::SharedString::from(entry.name().to_string())),
                    ),
            );

        if let Some(desc) = entry.description() {
            item = item.child(
                gpui::div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(gpui::SharedString::from(desc.to_string())),
            );
        }

        Some(item)
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
        match ix {
            Some(path) => self.base.set_selected(path.row),
            None => {
                // No selection: leave the stored selection; ListState passes
                // None on blur/empty. We intentionally do not panic.
            }
        }
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
        let Some(entry) = self.base.selected_item() else {
            tracing::debug!(target: LOG, "confirm with no selection");
            return;
        };
        tracing::info!(
            target: LOG,
            name = %entry.name(),
            "palette entry confirmed"
        );
        if let Some(cb) = &self.on_confirm {
            cb(entry);
        }
    }

    fn cancel(&mut self, _window: &mut Window, _cx: &mut Context<ListState<Self>>) {
        tracing::debug!(target: LOG, "palette cancelled");
        if let Some(cb) = &self.on_cancel {
            cb();
        }
    }
}
