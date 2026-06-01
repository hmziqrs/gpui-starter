//! Network connectivity mode for queries.
//!
//! [`NetworkMode`] controls whether queries should fetch based on network
//! connectivity state. Inspired by TanStack Query's `networkMode` option.

use serde::{Deserialize, Serialize};

/// Network connectivity mode for queries.
///
/// Controls whether queries attempt to fetch based on the perceived network
/// state. For desktop applications, the default `Online` mode is typically
/// appropriate since the network is usually available.
///
/// # Example
///
/// ```
/// use gpui_query::core::NetworkMode;
///
/// let mode = NetworkMode::Always;
/// assert_eq!(mode.label(), "always");
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NetworkMode {
    /// Only fetch when online (default for web, less relevant for desktop).
    #[default]
    Online,
    /// Always fetch regardless of connectivity.
    Always,
    /// Use a custom connectivity check (offline-first strategy).
    OfflineFirst,
}

impl NetworkMode {
    /// Returns a static label string for this network mode.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Always => "always",
            Self::OfflineFirst => "offline-first",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_online() {
        assert_eq!(NetworkMode::default(), NetworkMode::Online);
    }

    #[test]
    fn labels() {
        assert_eq!(NetworkMode::Online.label(), "online");
        assert_eq!(NetworkMode::Always.label(), "always");
        assert_eq!(NetworkMode::OfflineFirst.label(), "offline-first");
    }

    #[test]
    fn serde_roundtrip() {
        let mode = NetworkMode::OfflineFirst;
        let json = serde_json::to_string(&mode).unwrap();
        let back: NetworkMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, back);
    }
}
