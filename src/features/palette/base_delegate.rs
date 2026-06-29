//! Generic selection-state helper for list delegates.
//!
//! A view-agnostic, framework-independent store of "all items / filtered
//! indices / selected index / query" state. It is deliberately decoupled from
//! both GPUI and the [`gpui_component::list::ListDelegate`] trait so it can be
//! unit-tested in isolation and reused by any palette-style view.
//!
//! Ported from the reference launcher's `BaseDelegate<T>` but rebound to gpui-starter
//! conventions: no `.unwrap()`/`.expect()` in non-test paths, an idiomatic
//! tracing `LOG` target, and no launcher-specific item types.

const LOG: &str = "gpui_starter::palette::base_delegate";

/// Common state and behaviour shared by palette delegates.
///
/// Holds the full item set, the currently visible (filtered) subset as indices
/// into that set, an optional selection, and the active query string. All
/// mutation helpers are infallible: out-of-range inputs are clamped or ignored
/// and logged, never panicked.
///
/// # Type parameters
///
/// - `T`: the item type stored in the list. Bounded by `Clone` so delegates can
///   hand out owned copies without aliasing the internal slice.
pub struct BaseDelegate<T: Clone> {
    /// The complete, unfiltered item set.
    items: Vec<T>,
    /// Indices into [`BaseDelegate::items`] that are currently visible.
    filtered_indices: Vec<usize>,
    /// Currently selected position inside the *filtered* view (`None` = empty).
    selected_index: Option<usize>,
    /// The active search query.
    query: String,
}

impl<T: Clone> BaseDelegate<T> {
    /// Create a new base delegate owning `items`.
    ///
    /// Initialises the filtered view to "everything visible" and selects the
    /// first item when the set is non-empty.
    pub fn new(items: Vec<T>) -> Self {
        let len = items.len();
        let filtered_indices: Vec<usize> = (0..len).collect();
        Self {
            items,
            filtered_indices,
            selected_index: if len > 0 { Some(0) } else { None },
            query: String::new(),
        }
    }

    /// The currently selected filtered position, if any.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Clamp-aware selection setter.
    ///
    /// Sets the selection to `index` only when it lies within the filtered
    /// view; otherwise the request is ignored and logged at debug level.
    pub fn set_selected(&mut self, index: usize) {
        if index < self.filtered_count() {
            self.selected_index = Some(index);
        } else {
            tracing::debug!(
                target: LOG,
                index,
                count = self.filtered_count(),
                "set_selected ignored: out of range"
            );
        }
    }

    /// Set the selection without bounds checking.
    ///
    /// Intended for delegates that manage extra items beyond the filtered set
    /// (e.g. synthetic header rows). Callers are responsible for validity.
    pub fn set_selected_unchecked(&mut self, index: usize) {
        self.selected_index = Some(index);
    }

    /// Number of currently visible (filtered) items.
    pub fn filtered_count(&self) -> usize {
        self.filtered_indices.len()
    }

    /// Borrow the active query.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Store a new query. The caller is expected to re-run filtering afterwards.
    pub fn set_query(&mut self, query: String) {
        self.query = query;
    }

    /// Clear the query and reset the filtered view to "all visible".
    pub fn clear_query(&mut self) {
        self.query.clear();
        self.reset_filter();
    }

    /// Reset the filtered view so every item is visible, re-selecting the first.
    pub fn reset_filter(&mut self) {
        self.filtered_indices = (0..self.items.len()).collect();
        self.selected_index = if self.filtered_indices.is_empty() {
            None
        } else {
            Some(0)
        };
    }

    /// Replace the filtered view with `indices` and re-select the first item.
    ///
    /// Used after a background/synchronous filter pass.
    pub fn apply_filtered_indices(&mut self, indices: Vec<usize>) {
        self.filtered_indices = indices;
        self.selected_index = if self.filtered_indices.is_empty() {
            None
        } else {
            Some(0)
        };
    }

    /// Fetch an item by its *filtered* position.
    pub fn get_filtered_item(&self, filtered_index: usize) -> Option<&T> {
        self.filtered_indices
            .get(filtered_index)
            .and_then(|&item_idx| self.items.get(item_idx))
    }

    /// The currently selected item, if any.
    pub fn selected_item(&self) -> Option<&T> {
        self.selected_index
            .and_then(|idx| self.get_filtered_item(idx))
    }

    /// Move the selection down by one, wrapping at the bottom. No-op when empty.
    pub fn select_down(&mut self) {
        let count = self.filtered_count();
        if count == 0 {
            return;
        }
        let current = self.selected_index.unwrap_or(0);
        let next = if current + 1 >= count { 0 } else { current + 1 };
        self.selected_index = Some(next);
    }

    /// Move the selection up by one, wrapping at the top. No-op when empty.
    pub fn select_up(&mut self) {
        let count = self.filtered_count();
        if count == 0 {
            return;
        }
        let current = self.selected_index.unwrap_or(0);
        let prev = if current == 0 { count - 1 } else { current - 1 };
        self.selected_index = Some(prev);
    }

    /// Borrow the full, unfiltered item set.
    pub fn items(&self) -> &[T] {
        &self.items
    }

    /// Borrow the current filtered indices.
    pub fn filtered_indices(&self) -> &[usize] {
        &self.filtered_indices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_initialises_filter_and_selection() {
        let d = BaseDelegate::new(vec!["a", "b", "c"]);
        assert_eq!(d.filtered_count(), 3);
        assert_eq!(d.selected_index(), Some(0));
    }

    #[test]
    fn empty_set_has_no_selection() {
        let d: BaseDelegate<&str> = BaseDelegate::new(vec![]);
        assert_eq!(d.selected_index(), None);
        assert_eq!(d.filtered_count(), 0);
    }

    #[test]
    fn navigation_wraps() {
        let mut d = BaseDelegate::new(vec!["a", "b", "c"]);
        d.select_down();
        d.select_down();
        assert_eq!(d.selected_index(), Some(2));
        d.select_down(); // wraps to 0
        assert_eq!(d.selected_index(), Some(0));
        d.select_up(); // wraps to last
        assert_eq!(d.selected_index(), Some(2));
    }

    #[test]
    fn apply_filtered_indices_resets_selection() {
        let mut d = BaseDelegate::new(vec!["a", "b", "c", "d"]);
        d.apply_filtered_indices(vec![1, 3]);
        assert_eq!(d.selected_index(), Some(0));
        assert_eq!(d.selected_item(), Some(&"b"));
        assert_eq!(d.filtered_count(), 2);
    }

    #[test]
    fn set_selected_ignores_out_of_range() {
        let mut d = BaseDelegate::new(vec!["a", "b"]);
        d.set_selected(99);
        assert_eq!(d.selected_index(), Some(0)); // unchanged
    }

    #[test]
    fn clear_query_restores_full_view() {
        let mut d = BaseDelegate::new(vec!["a", "b", "c"]);
        d.apply_filtered_indices(vec![1]);
        assert_eq!(d.filtered_count(), 1);
        d.clear_query();
        assert_eq!(d.filtered_count(), 3);
    }
}
