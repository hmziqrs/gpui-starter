use std::sync::Arc;

use gpui::{Global, SharedString};

use super::types::{
    NotificationBackendKind, NotificationCapabilities, NotificationImportance,
    NotificationPermissionState, NotificationRequest, NotificationSendResult,
};
use super::{NotificationBackend, NotifyRustBackend, UserNotifyBackend};

pub const LOG: &str = "gpui_starter::notifications";

#[derive(Clone, Debug)]
pub struct NotificationRuntimeSnapshot {
    pub enabled_by_user: bool,
    pub permission: NotificationPermissionState,
    pub active_backend: NotificationBackendKind,
    pub capabilities: NotificationCapabilities,
    pub last_backend_error: Option<SharedString>,
    pub degraded_reason: Option<SharedString>,
}

impl NotificationRuntimeSnapshot {
    pub fn new(service: &NotificationService) -> Self {
        Self {
            enabled_by_user: true,
            permission: NotificationPermissionState::Unknown,
            active_backend: service.active_backend(),
            capabilities: service.active_capabilities(),
            last_backend_error: service.initial_error.clone().map(Into::into),
            degraded_reason: service.initial_error.clone().map(Into::into),
        }
    }
}

#[derive(Clone)]
pub struct NativeNotificationState {
    pub service: Arc<NotificationService>,
    pub snapshot: NotificationRuntimeSnapshot,
}

impl Global for NativeNotificationState {}

pub struct NotificationService {
    pub(crate) primary: Option<Arc<dyn NotificationBackend>>,
    pub(crate) secondary: Arc<dyn NotificationBackend>,
    pub(crate) initial_error: Option<String>,
}

impl NotificationService {
    pub fn new() -> Self {
        tracing::info!(target: LOG, "initializing native notification service");

        let mut initial_error = None;
        let primary = match UserNotifyBackend::new() {
            Ok(backend) => {
                tracing::info!(target: LOG, backend = %NotificationBackendKind::UserNotify, "primary notification backend selected");
                Some(Arc::new(backend) as Arc<dyn NotificationBackend>)
            }
            Err(err) => {
                tracing::warn!(
                    target: LOG,
                    backend = %NotificationBackendKind::UserNotify,
                    error = %err,
                    "primary notification backend unavailable; falling back"
                );
                initial_error = Some(err.to_string());
                None
            }
        };

        let service = Self {
            primary,
            secondary: Arc::new(NotifyRustBackend::new()),
            initial_error,
        };

        tracing::info!(
            target: LOG,
            active_backend = %service.active_backend(),
            capabilities = ?service.active_capabilities(),
            degraded_reason = ?service.initial_error,
            "native notification service initialized"
        );

        service
    }

    pub(super) fn active_backend(&self) -> NotificationBackendKind {
        self.primary
            .as_ref()
            .map(|backend| backend.kind())
            .unwrap_or(NotificationBackendKind::NotifyRust)
    }

    pub(super) fn active_capabilities(&self) -> NotificationCapabilities {
        self.primary
            .as_ref()
            .map(|backend| backend.capabilities())
            .unwrap_or_else(|| self.secondary.capabilities())
    }

    pub(super) async fn refresh_permission_state(&self) -> NotificationPermissionState {
        tracing::debug!(
            target: LOG,
            active_backend = %self.active_backend(),
            "refreshing notification permission state"
        );

        if let Some(primary) = &self.primary {
            primary.refresh_permission_state().await
        } else if cfg!(target_os = "macos") {
            let state = NotificationPermissionState::Unavailable(
                self.initial_error
                    .clone()
                    .unwrap_or_else(|| "primary backend unavailable".to_string()),
            );
            tracing::info!(target: LOG, ?state, "permission state unavailable without primary backend");
            state
        } else {
            tracing::info!(target: LOG, "permission state unsupported on this platform");
            NotificationPermissionState::Unsupported
        }
    }

    pub(super) async fn request_permission(&self) -> NotificationPermissionState {
        tracing::info!(
            target: LOG,
            active_backend = %self.active_backend(),
            "requesting notification permission"
        );

        if let Some(primary) = &self.primary {
            primary.request_permission().await
        } else if cfg!(target_os = "macos") {
            let state = NotificationPermissionState::Unavailable(
                self.initial_error
                    .clone()
                    .unwrap_or_else(|| "primary backend unavailable".to_string()),
            );
            tracing::warn!(target: LOG, ?state, "cannot request permission without primary backend");
            state
        } else {
            tracing::info!(target: LOG, "permission request unsupported on this platform");
            NotificationPermissionState::Unsupported
        }
    }

    pub async fn send(
        &self,
        request: NotificationRequest,
        enabled_by_user: bool,
    ) -> NotificationSendResult {
        tracing::info!(
            target: LOG,
            title = %request.title,
            importance = %request.importance,
            prefer_native = request.prefer_native,
            enabled_by_user,
            active_backend = %self.active_backend(),
            "native notification send requested"
        );

        if !enabled_by_user || !request.prefer_native {
            tracing::warn!(
                target: LOG,
                enabled_by_user,
                prefer_native = request.prefer_native,
                importance = %request.importance,
                "native send skipped; using in-app policy"
            );
            return NotificationSendResult {
                backend_used: NotificationBackendKind::UiOnly,
                degraded: request.importance == NotificationImportance::BackgroundWorthy,
                delivered_natively: false,
                error_summary: Some("native notifications are disabled".into()),
                importance: request.importance,
            };
        }

        let mut errors = Vec::new();

        if let Some(primary) = &self.primary {
            tracing::debug!(target: LOG, backend = %primary.kind(), "attempting primary notification send");
            match primary.send(&request).await {
                Ok(()) => {
                    tracing::info!(target: LOG, backend = %primary.kind(), "primary notification send succeeded");
                    return NotificationSendResult {
                        backend_used: primary.kind(),
                        degraded: false,
                        delivered_natively: true,
                        error_summary: None,
                        importance: request.importance,
                    };
                }
                Err(err) => {
                    tracing::warn!(
                        target: LOG,
                        backend = %primary.kind(),
                        error = %err,
                        "primary notification send failed"
                    );
                    errors.push(format!("{}: {err:#}", primary.kind()));
                }
            }
        }

        tracing::debug!(
            target: LOG,
            backend = %self.secondary.kind(),
            "attempting fallback notification send"
        );
        match self.secondary.send(&request).await {
            Ok(()) => {
                tracing::info!(
                    target: LOG,
                    backend = %self.secondary.kind(),
                    degraded = self.primary.is_some(),
                    "fallback notification send succeeded"
                );
                NotificationSendResult {
                    backend_used: self.secondary.kind(),
                    degraded: self.primary.is_some(),
                    delivered_natively: true,
                    error_summary: if errors.is_empty() {
                        None
                    } else {
                        Some(errors.join("; ").into())
                    },
                    importance: request.importance,
                }
            }
            Err(err) => {
                tracing::warn!(
                    target: LOG,
                    backend = %self.secondary.kind(),
                    error = %err,
                    "fallback notification send failed; using in-app policy"
                );
                errors.push(format!("{}: {err:#}", self.secondary.kind()));
                NotificationSendResult {
                    backend_used: NotificationBackendKind::UiOnly,
                    degraded: true,
                    delivered_natively: false,
                    error_summary: Some(errors.join("; ").into()),
                    importance: request.importance,
                }
            }
        }
    }
}
