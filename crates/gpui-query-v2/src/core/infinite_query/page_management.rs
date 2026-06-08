//! Page management methods for [`InfiniteQueryResource`].

use super::{FetchDirection, InfiniteQueryResource};

// ── Page management ─────────────────────────────────────────────────────

impl<T, E> InfiniteQueryResource<T, E> {
    /// Set whether more pages are available after the last loaded page.
    pub fn set_has_next_page(&mut self, has_next: bool) {
        self.has_next_page = has_next;
    }

    /// Set whether more pages are available before the first loaded page.
    pub fn set_has_previous_page(&mut self, has_prev: bool) {
        self.has_previous_page = has_prev;
    }

    /// Set the fetch direction mode.
    ///
    /// This does not change the current `has_next_page` / `has_previous_page`
    /// flags — it only affects what `reset()` restores them to.
    pub fn set_direction(&mut self, direction: FetchDirection) {
        self.direction = direction;
    }

    /// Set the maximum number of pages to retain.
    ///
    /// A value of `Some(0)` is treated as unbounded (`None`) to prevent
    /// accidentally draining all pages. Callers that want no page retention
    /// should use `reset()` instead.
    ///
    /// Returns evicted pages (if any) so the caller can log or process them.
    pub fn set_max_pages(&mut self, max: Option<usize>) -> Vec<T> {
        // Treat 0 as unbounded to prevent draining all pages.
        self.max_pages = match max {
            Some(0) => None,
            other => other,
        };
        self.enforce_max_pages_remove_front()
    }

    /// Append a page to the end.
    ///
    /// **Audit 3**: Uses `VecDeque::push_back` — O(1) amortized.
    ///
    /// Returns evicted pages (if any) so the caller can log or process them.
    pub fn append_page(&mut self, page: T) -> Vec<T> {
        self.pages.push_back(page);
        self.enforce_max_pages_remove_front()
    }

    /// Prepend a page to the beginning.
    ///
    /// **Audit 3**: Uses `VecDeque::push_front` — O(1) amortized instead of
    /// the previous `Vec::insert(0, page)` which was O(n).
    ///
    /// Returns evicted pages (if any) so the caller can log or process them.
    pub fn prepend_page(&mut self, page: T) -> Vec<T> {
        self.pages.push_front(page);
        self.enforce_max_pages_remove_back()
    }

    /// **v2 fix**: Use `Vec::drain` instead of O(n²) `remove(0)`.
    ///
    /// **Audit 2 fix**: `max_pages` of 0 is treated as unbounded. At least 1
    /// page is always retained. Returns evicted pages for caller inspection.
    ///
    /// **Audit 3**: Uses `VecDeque::drain` — O(k) where k is the number of
    /// evicted pages.
    pub(super) fn enforce_max_pages_remove_front(&mut self) -> Vec<T> {
        if let Some(max) = self.max_pages {
            if max > 0 && self.pages.len() > max {
                return self.pages.drain(..self.pages.len() - max).collect();
            }
        }
        Vec::new()
    }

    /// Evict pages from the back until within `max_pages`.
    ///
    /// **Audit 2 fix**: `max_pages` of 0 is treated as unbounded. At least 1
    /// page is always retained. Returns evicted pages for caller inspection.
    ///
    /// **Audit 3**: Uses `VecDeque::pop_back` — O(1) per eviction.
    pub(super) fn enforce_max_pages_remove_back(&mut self) -> Vec<T> {
        let mut evicted = Vec::new();
        if let Some(max) = self.max_pages {
            if max > 0 {
                while self.pages.len() > max {
                    if let Some(page) = self.pages.pop_back() {
                        evicted.push(page);
                    }
                }
            }
        }
        evicted
    }
}
