//! Clipboard history store.
//!
//! An in-memory ring buffer of recent clipboard contents with consecutive
//! deduplication and a configurable capacity cap. The store is generic
//! (`ClipboardContent`) and carries no launcher coupling.
//!
//! Concurrency model: a single `RwLock` guards the buffer. Acquisition
//! failures are logged and degraded to a no-op rather than panicking, in
//! keeping with the boilerplate's "log + degrade" convention.

use std::collections::VecDeque;
use std::sync::{PoisonError, RwLock};

use super::item::{ClipboardContent, ClipboardItem};

const LOG: &str = "gpui_starter::clipboard::data";

/// Default maximum number of entries retained in the history.
pub const DEFAULT_CAPACITY: usize = 512;

/// In-memory clipboard history.
///
/// Newer entries live at the front of the deque. Pushing an item identical
/// to the current front is a no-op (consecutive dedup). Once the buffer is
/// at capacity the oldest entry (back) is evicted.
pub struct ClipboardHistory {
    entries: RwLock<VecDeque<ClipboardItem>>,
    capacity: usize,
}

impl ClipboardHistory {
    /// Create a new, empty history with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: RwLock::new(VecDeque::with_capacity(capacity.min(64))),
            capacity,
        }
    }

    /// Create a new, empty history with [`DEFAULT_CAPACITY`].
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// Current configured capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Append a new entry to the front of the history.
    ///
    /// Returns `true` when an item was actually added and `false` when it
    /// was suppressed as a consecutive duplicate or when the lock was
    /// poisoned (the failure is logged and degraded).
    pub fn push(&self, content: ClipboardContent) -> bool {
        let mut guard = match self.entries.write() {
            Ok(g) => g,
            Err(err) => {
                log_poisoned("push", err);
                return false;
            }
        };

        // Consecutive dedup: drop the new entry if it is identical to the
        // most recent one. This mirrors the upstream reference behaviour while
        // staying allocation-free on the common copy-the-same-thing path.
        if let Some(front) = guard.front()
            && front.content == content
        {
            return false;
        }

        let item = ClipboardItem::new(content);
        guard.push_front(item);

        if guard.len() > self.capacity {
            guard.pop_back();
        }
        true
    }

    /// Number of entries currently held.
    pub fn len(&self) -> usize {
        match self.entries.read() {
            Ok(g) => g.len(),
            Err(err) => {
                log_poisoned("len", err);
                0
            }
        }
    }

    /// Whether the history is empty. Lock poisoning is treated as empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return a snapshot of the history, oldest-last, optionally filtered by
    /// a (case-insensitive, substring) search query applied to text entries.
    ///
    /// Image entries are always included when the query is non-empty to keep
    /// the call total (search query, non-text items) predictable for the UI.
    pub fn snapshot(&self, query: &str) -> Vec<ClipboardItem> {
        let guard = match self.entries.read() {
            Ok(g) => g,
            Err(err) => {
                log_poisoned("snapshot", err);
                return Vec::new();
            }
        };

        if query.is_empty() {
            return guard.iter().cloned().collect();
        }

        let needle = query.to_lowercase();
        guard
            .iter()
            .filter(|item| match &item.content {
                ClipboardContent::Text(text) => text.to_lowercase().contains(&needle),
                ClipboardContent::Image { .. } => true,
            })
            .cloned()
            .collect()
    }

    /// Remove every entry from the history.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.entries.write() {
            guard.clear();
        }
    }
}

impl Default for ClipboardHistory {
    fn default() -> Self {
        Self::new()
    }
}

fn log_poisoned<T>(op: &str, err: PoisonError<T>) {
    tracing::error!(
        target: LOG,
        op,
        error = %err,
        "clipboard history lock poisoned; degrading to no-op"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_adds_new_entries_to_front() {
        let hist = ClipboardHistory::with_capacity(8);
        assert!(hist.push(ClipboardContent::Text("a".into())));
        assert!(hist.push(ClipboardContent::Text("b".into())));
        let snap = hist.snapshot("");
        assert_eq!(snap.len(), 2);
        assert!(matches!(
            &snap[0].content,
            ClipboardContent::Text(t) if t == "b"
        ));
    }

    #[test]
    fn consecutive_duplicates_are_dropped() {
        let hist = ClipboardHistory::with_capacity(8);
        assert!(hist.push(ClipboardContent::Text("a".into())));
        assert!(!hist.push(ClipboardContent::Text("a".into())));
        assert_eq!(hist.len(), 1);
    }

    #[test]
    fn capacity_cap_evicts_oldest() {
        let hist = ClipboardHistory::with_capacity(2);
        hist.push(ClipboardContent::Text("a".into()));
        hist.push(ClipboardContent::Text("b".into()));
        hist.push(ClipboardContent::Text("c".into()));
        assert_eq!(hist.len(), 2);
        let snap = hist.snapshot("");
        assert!(matches!(
            &snap[1].content,
            ClipboardContent::Text(t) if t == "b"
        ));
    }

    #[test]
    fn snapshot_filters_by_case_insensitive_query() {
        let hist = ClipboardHistory::with_capacity(8);
        hist.push(ClipboardContent::Text("Hello World".into()));
        hist.push(ClipboardContent::Text("goodbye".into()));
        let matched = hist.snapshot("hello");
        assert_eq!(matched.len(), 1);
    }

    #[test]
    fn clear_empties_history() {
        let hist = ClipboardHistory::with_capacity(8);
        hist.push(ClipboardContent::Text("a".into()));
        hist.clear();
        assert!(hist.is_empty());
    }
}
