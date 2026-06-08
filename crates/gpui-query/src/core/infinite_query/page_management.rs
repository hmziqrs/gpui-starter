use crate::core::InfiniteQueryResource;

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

    /// Set the maximum number of pages to retain.
    ///
    /// When set, appending or prepending pages beyond this limit causes the
    /// oldest pages to be dropped. Immediately trims if the current page
    /// count already exceeds the new limit (removes from the front).
    pub fn set_max_pages(&mut self, max: Option<usize>) {
        self.max_pages = max;
        self.enforce_max_pages_remove_front();
    }

    /// Append a new page of data to the end of the pages list.
    ///
    /// When `max_pages` is set and exceeded, removes pages from the front
    /// (oldest pages for forward pagination).
    pub fn append_page(&mut self, page: T) {
        self.pages.push(page);
        self.enforce_max_pages_remove_front();
    }

    /// Prepend a page of data to the beginning of the pages list.
    ///
    /// Useful for bidirectional pagination (loading older content above).
    /// When `max_pages` is set and exceeded, removes pages from the back
    /// (oldest pages for backward pagination).
    pub fn prepend_page(&mut self, page: T) {
        self.pages.insert(0, page);
        self.enforce_max_pages_remove_back();
    }

    /// Remove pages from the front (oldest in forward direction) beyond `max_pages`.
    pub(crate) fn enforce_max_pages_remove_front(&mut self) {
        if let Some(max) = self.max_pages {
            while self.pages.len() > max {
                self.pages.remove(0);
            }
        }
    }

    /// Remove pages from the back (oldest in backward direction) beyond `max_pages`.
    pub(crate) fn enforce_max_pages_remove_back(&mut self) {
        if let Some(max) = self.max_pages {
            while self.pages.len() > max {
                self.pages.pop();
            }
        }
    }
}
