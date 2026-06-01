//! Refetch trigger configuration for queries.
//!
//! [`RefetchTrigger`] controls when queries should automatically refetch
//! data. Inspired by TanStack Query's `refetchOnMount`, `refetchOnWindowFocus`,
//! and `refetchOnReconnect` options.

/// Refetch trigger configuration.
///
/// Controls when a query should automatically refetch its data.
/// The `OnWindowFocus` and `OnReconnect` variants accept an optional
/// `stale_time_ms` parameter — if set, refetching only occurs when the
/// data is older than the specified stale time.
///
/// # Example
///
/// ```
/// use gpui_query::core::RefetchTrigger;
///
/// let trigger = RefetchTrigger::OnWindowFocus { stale_time_ms: Some(30_000) };
/// assert_eq!(trigger.label(), "on-window-focus");
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum RefetchTrigger {
    /// Never automatically refetch.
    Never,
    /// Refetch when the component mounts.
    OnMount,
    /// Refetch when the window gains focus (if data is stale).
    OnWindowFocus {
        /// Only refetch if data is older than this many milliseconds.
        stale_time_ms: Option<u64>,
    },
    /// Refetch when connectivity is restored.
    OnReconnect {
        /// Only refetch if data is older than this many milliseconds.
        stale_time_ms: Option<u64>,
    },
    /// Always refetch on any trigger.
    Always,
}

impl RefetchTrigger {
    /// Returns a static label string for this refetch trigger.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::OnMount => "on-mount",
            Self::OnWindowFocus { .. } => "on-window-focus",
            Self::OnReconnect { .. } => "on-reconnect",
            Self::Always => "always",
        }
    }
}

impl Default for RefetchTrigger {
    fn default() -> Self {
        Self::OnMount
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_on_mount() {
        assert_eq!(RefetchTrigger::default(), RefetchTrigger::OnMount);
    }

    #[test]
    fn labels() {
        assert_eq!(RefetchTrigger::Never.label(), "never");
        assert_eq!(RefetchTrigger::OnMount.label(), "on-mount");
        assert_eq!(
            RefetchTrigger::OnWindowFocus { stale_time_ms: None }.label(),
            "on-window-focus"
        );
        assert_eq!(
            RefetchTrigger::OnReconnect { stale_time_ms: Some(1000) }.label(),
            "on-reconnect"
        );
        assert_eq!(RefetchTrigger::Always.label(), "always");
    }
}
