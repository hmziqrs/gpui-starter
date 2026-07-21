use std::collections::BTreeMap;

use gpui::{App, Global, SharedString};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityStatus {
    pub supported: bool,
    pub enabled: bool,
    pub degraded: bool,
    pub reason: Option<SharedString>,
    pub last_error: Option<SharedString>,
}

impl CapabilityStatus {
    pub fn supported_enabled() -> Self {
        Self {
            supported: true,
            enabled: true,
            degraded: false,
            reason: None,
            last_error: None,
        }
    }

    /// Capability is supported but a sub-component failed: the feature stays
    /// enabled (users can still exercise the parts that work) but is flagged
    /// degraded with `err` as both the human-readable reason and last_error.
    pub fn degraded(err: impl ToString) -> Self {
        let msg: SharedString = err.to_string().into();
        Self {
            supported: true,
            enabled: true,
            degraded: true,
            reason: Some(msg.clone()),
            last_error: Some(msg),
        }
    }

    /// Capability failed to initialize entirely: supported but not enabled,
    /// flagged degraded, with `err` as both the reason and last_error.
    pub fn error(err: impl ToString) -> Self {
        let msg: SharedString = err.to_string().into();
        Self {
            supported: true,
            enabled: false,
            degraded: true,
            reason: Some(msg.clone()),
            last_error: Some(msg),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CapabilityRegistry {
    entries: BTreeMap<String, CapabilityStatus>,
}

impl Global for CapabilityRegistry {}

pub fn initialize(cx: &mut App) {
    cx.set_global(CapabilityRegistry::default());
}

pub fn set(name: impl Into<String>, status: CapabilityStatus, cx: &mut App) {
    let key = name.into();
    tracing::debug!(
        target: "gpui_starter::capabilities",
        capability = %key,
        supported = status.supported,
        enabled = status.enabled,
        degraded = status.degraded,
        reason = ?status.reason,
        last_error = ?status.last_error,
        "capability updated"
    );
    cx.default_global::<CapabilityRegistry>()
        .entries
        .insert(key, status);
}

pub fn snapshot(cx: &App) -> BTreeMap<String, CapabilityStatus> {
    cx.try_global::<CapabilityRegistry>()
        .map(|registry| registry.entries.clone())
        .unwrap_or_default()
}
