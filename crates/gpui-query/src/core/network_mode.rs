//! Network mode configuration (forward compatibility).

use serde::{Deserialize, Serialize};

/// Controls fetch behavior based on network connectivity.
///
/// Note: In v2, this is defined for forward compatibility. The actual
/// network detection is not yet implemented.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkMode {
    /// Only fetch when online (default).
    #[default]
    Online,
    /// Always try to fetch, even when offline.
    Always,
    /// Use cached data first, fetch when online.
    OfflineFirst,
}

impl NetworkMode {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Always => "Always",
            Self::OfflineFirst => "Offline first",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_mode_labels() {
        assert_eq!(NetworkMode::Online.label(), "Online");
        assert_eq!(NetworkMode::Always.label(), "Always");
        assert_eq!(NetworkMode::OfflineFirst.label(), "Offline first");
    }

    #[test]
    fn network_mode_default_is_online() {
        assert_eq!(NetworkMode::default(), NetworkMode::Online);
    }

    #[test]
    fn network_mode_serde_roundtrip() {
        for mode in [NetworkMode::Online, NetworkMode::Always, NetworkMode::OfflineFirst] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: NetworkMode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, mode);
        }
    }

    #[test]
    fn network_mode_equality() {
        assert_eq!(NetworkMode::Online, NetworkMode::Online);
        assert_ne!(NetworkMode::Online, NetworkMode::Always);
        assert_ne!(NetworkMode::Always, NetworkMode::OfflineFirst);
    }
}
