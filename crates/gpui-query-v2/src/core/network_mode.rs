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
