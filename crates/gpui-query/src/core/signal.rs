//! Cooperative cancellation signal for in-flight query requests.
//!
//! [`QuerySignal`] uses a shared atomic flag so that all clones observe the
//! same cancellation state. The fetcher is expected to check `is_cancelled()`
//! periodically and abort early when possible.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A cooperative cancellation signal for in-flight query requests.
///
/// Clones share the same underlying flag, so cancelling any clone
/// cancels all of them.
#[derive(Debug, Clone)]
pub struct QuerySignal {
    cancelled: Arc<AtomicBool>,
}

impl PartialEq for QuerySignal {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.cancelled, &other.cancelled)
    }
}

impl Eq for QuerySignal {}

impl QuerySignal {
    /// Create a new, non-cancelled signal.
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Signal cancellation. All clones sharing this flag will observe
    /// `is_cancelled() == true` after this call.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Check whether cancellation has been signalled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl Default for QuerySignal {
    fn default() -> Self {
        Self::new()
    }
}
