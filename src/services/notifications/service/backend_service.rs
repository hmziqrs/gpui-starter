use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures_util::FutureExt;
use gpui::{Global, SharedString};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use super::UserNotifyBackend;
use super::types::{
    NotificationBackendKind, NotificationCapabilities, NotificationImportance,
    NotificationPermissionState, NotificationRequest, NotificationSendResult,
};
use super::{NotificationBackend, NotifyRustBackend};

pub const LOG: &str = "gpui_starter::notifications";

#[derive(Clone, Debug)]
pub struct NotificationRuntimeSnapshot {
    pub enabled_by_user: bool,
    pub permission: NotificationPermissionState,
    pub active_backend: NotificationBackendKind,
    pub capabilities: NotificationCapabilities,
    pub last_backend_error: Option<SharedString>,
    pub degraded_reason: Option<SharedString>,
    /// Advisory: the freedesktop capabilities the running daemon advertises
    /// (e.g. `body-markup`, `actions`, `icon-static`), probed at startup. Treated
    /// as advisory — servers may ignore hints they nominally advertise.
    pub daemon_capabilities: Option<SharedString>,
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
            daemon_capabilities: None,
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
        let secondary = Arc::new(NotifyRustBackend::new()) as Arc<dyn NotificationBackend>;
        let (primary, primary_error) = select_primary_backend();
        Self::with_backends(primary, secondary, primary_error)
    }

    /// Construct with explicit backends (primarily a test hook). `primary` is
    /// tried first; `secondary` is the fallback reached on any primary `Err` or
    /// panic.
    pub(crate) fn with_backends(
        primary: Option<Arc<dyn NotificationBackend>>,
        secondary: Arc<dyn NotificationBackend>,
        initial_error: Option<String>,
    ) -> Self {
        let service = Self {
            primary,
            secondary,
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
            // catch_unwind: a panic inside a backend (e.g. notify-rust's
            // action-signal `.unwrap()` chain, reached only after a successful
            // show) would otherwise abort the whole send task and bypass the
            // secondary fallback. Convert a panic into a regular error so the
            // fallback is always attempted.
            match AssertUnwindSafe(primary.send(&request))
                .catch_unwind()
                .await
            {
                Ok(Ok(())) => {
                    tracing::info!(target: LOG, backend = %primary.kind(), "primary notification send succeeded");
                    return NotificationSendResult {
                        backend_used: primary.kind(),
                        degraded: false,
                        delivered_natively: true,
                        error_summary: None,
                        importance: request.importance,
                    };
                }
                Ok(Err(err)) => {
                    tracing::warn!(
                        target: LOG,
                        backend = %primary.kind(),
                        error = %err,
                        "primary notification send failed"
                    );
                    errors.push(format!("{}: {err:#}", primary.kind()));
                }
                Err(panic_payload) => {
                    let msg = panic_payload_to_string(panic_payload);
                    tracing::error!(
                        target: LOG,
                        backend = %primary.kind(),
                        panic = %msg,
                        "primary notification send panicked; falling back to secondary"
                    );
                    errors.push(format!("{}: panic: {msg}", primary.kind()));
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

/// Select the preferred (primary) notification backend for this platform, or
/// `None` to let the robust synchronous NotifyRustBackend be the active path
/// (Linux native). `primary_error` records why a preferred backend was wanted
/// but unavailable.
fn select_primary_backend() -> (Option<Arc<dyn NotificationBackend>>, Option<String>) {
    // macOS / Windows: user-notify gives the richest native experience.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        match UserNotifyBackend::new() {
            Ok(backend) => {
                tracing::info!(
                    target: LOG,
                    backend = %NotificationBackendKind::UserNotify,
                    "primary notification backend selected"
                );
                return (
                    Some(Arc::new(backend) as Arc<dyn NotificationBackend>),
                    None,
                );
            }
            Err(err) => {
                tracing::warn!(
                    target: LOG,
                    backend = %NotificationBackendKind::UserNotify,
                    error = %err,
                    "primary notification backend unavailable; falling back"
                );
                return (None, Some(err.to_string()));
            }
        }
    }

    // Sandboxed (Flatpak/Snap) Linux + the opt-in portal feature: route through
    // the XDG Desktop Portal instead of org.freedesktop.Notifications directly.
    #[cfg(all(feature = "notifications-portal", target_os = "linux"))]
    if crate::platform::environment::is_sandboxed() {
        tracing::info!(
            target: LOG,
            backend = %NotificationBackendKind::Portal,
            "sandbox detected; selecting XDG portal backend"
        );
        return (
            Some(Arc::new(super::PortalBackend::new()) as Arc<dyn NotificationBackend>),
            None,
        );
    } else {
        tracing::info!(
            target: LOG,
            "native (non-sandboxed) Linux; notify-rust (FreeDesktop/D-Bus) is the active backend"
        );
    }

    // Linux native (or portal feature off): NotifyRust is the active backend.
    // On macOS/Windows control always returned from the cfg block above, so the
    // tail is gated to non-macOS/non-Windows to keep it from being flagged
    // unreachable on those targets.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        (None, None)
    }
}

/// Best-effort stringification of a `catch_unwind` panic payload.
fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::NotificationService;
    use crate::services::notifications::backend::{NotificationBackend, NotifyRustBackend};
    use crate::services::notifications::service::types::{
        NotificationBackendKind, NotificationCapabilities, NotificationPermissionState,
        NotificationRequest,
    };

    use std::sync::Arc;

    use async_trait::async_trait;

    /// What a fake backend does when `send` is called.
    enum SendOutcome {
        Ok,
        Err(&'static str),
        Panic(&'static str),
    }

    struct FakeBackend {
        kind: NotificationBackendKind,
        outcome: SendOutcome,
    }

    #[async_trait]
    impl NotificationBackend for FakeBackend {
        fn kind(&self) -> NotificationBackendKind {
            self.kind
        }
        fn capabilities(&self) -> NotificationCapabilities {
            NotificationCapabilities::default()
        }
        async fn refresh_permission_state(&self) -> NotificationPermissionState {
            NotificationPermissionState::Unsupported
        }
        async fn request_permission(&self) -> NotificationPermissionState {
            NotificationPermissionState::Unsupported
        }
        async fn send(&self, _request: &NotificationRequest) -> anyhow::Result<()> {
            match self.outcome {
                SendOutcome::Ok => Ok(()),
                SendOutcome::Err(msg) => Err(anyhow::anyhow!("{msg}")),
                SendOutcome::Panic(msg) => panic!("{msg}"),
            }
        }
    }

    fn fake(kind: NotificationBackendKind, outcome: SendOutcome) -> Arc<dyn NotificationBackend> {
        Arc::new(FakeBackend { kind, outcome })
    }

    fn request() -> NotificationRequest {
        NotificationRequest::foreground("test title", "test body")
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().expect("failed to build tokio runtime for test")
    }

    #[test]
    fn fallback_used_when_primary_returns_err() {
        let svc = NotificationService::with_backends(
            Some(fake(
                NotificationBackendKind::UserNotify,
                SendOutcome::Err("primary boom"),
            )),
            fake(NotificationBackendKind::NotifyRust, SendOutcome::Ok),
            None,
        );
        let result = runtime().block_on(async { svc.send(request(), true).await });
        assert!(result.delivered_natively, "secondary should deliver");
        assert_eq!(result.backend_used, NotificationBackendKind::NotifyRust);
        assert!(
            result.degraded,
            "a primary existed, so the result is degraded"
        );
        let summary = result
            .error_summary
            .expect("should carry the primary error");
        assert!(
            summary.contains("primary boom"),
            "error summary should include the primary failure: {summary}"
        );
    }

    #[test]
    fn degrades_to_ui_only_when_both_backends_fail() {
        let svc = NotificationService::with_backends(
            Some(fake(
                NotificationBackendKind::UserNotify,
                SendOutcome::Err("primary-fail"),
            )),
            fake(
                NotificationBackendKind::NotifyRust,
                SendOutcome::Err("secondary-fail"),
            ),
            None,
        );
        let result = runtime().block_on(async { svc.send(request(), true).await });
        assert!(!result.delivered_natively);
        assert_eq!(result.backend_used, NotificationBackendKind::UiOnly);
        assert!(result.degraded);
        let summary = result
            .error_summary
            .expect("both backend errors are recorded");
        assert!(
            summary.contains("primary-fail") && summary.contains("secondary-fail"),
            "error summary should include both failures: {summary}"
        );
    }

    #[test]
    fn primary_panic_does_not_bypass_fallback() {
        // The whole point of catch_unwind: a panicking primary must not abort the
        // send task, and the secondary must still be tried.
        let svc = NotificationService::with_backends(
            Some(fake(
                NotificationBackendKind::UserNotify,
                SendOutcome::Panic("primary panicked"),
            )),
            fake(NotificationBackendKind::NotifyRust, SendOutcome::Ok),
            None,
        );
        let result = runtime().block_on(async { svc.send(request(), true).await });
        assert!(
            result.delivered_natively,
            "secondary must deliver despite the primary panic"
        );
        assert_eq!(result.backend_used, NotificationBackendKind::NotifyRust);
        let summary = result
            .error_summary
            .expect("panic must be recorded as an error");
        assert!(
            summary.to_lowercase().contains("panic"),
            "error summary should mention the panic: {summary}"
        );
    }

    #[test]
    fn native_send_skipped_when_disabled_by_user() {
        let svc = NotificationService::with_backends(
            Some(fake(NotificationBackendKind::UserNotify, SendOutcome::Ok)),
            fake(NotificationBackendKind::NotifyRust, SendOutcome::Ok),
            None,
        );
        let result = runtime().block_on(async { svc.send(request(), false).await });
        assert!(!result.delivered_natively);
        assert_eq!(result.backend_used, NotificationBackendKind::UiOnly);
    }

    /// On Linux the user-notify (async/xdg) backend is intentionally skipped: it
    /// offers nothing over the synchronous NotifyRustBackend and removes the
    /// async-vs-sync ambiguity. `new()` must therefore leave `primary = None` and
    /// report notify-rust as the active backend.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_selects_notify_rust_as_active_backend() {
        let svc = NotificationService::new();
        assert!(svc.primary.is_none(), "primary must be None on Linux");
        assert_eq!(svc.active_backend(), NotificationBackendKind::NotifyRust);
    }

    /// Linux/FreeDesktop has no per-app notification permission model, so the
    /// notify-rust backend must report `Unsupported` (never `Denied`/`Authorized`).
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_notify_rust_reports_unsupported_permission() {
        let backend = NotifyRustBackend::new();
        let state = runtime().block_on(async { backend.refresh_permission_state().await });
        assert_eq!(state, NotificationPermissionState::Unsupported);
    }

    /// The XDG portal backend (feature-gated) reports its kind + capabilities and
    /// constructs with no runtime handle (ashpd drives zbus's own thread).
    #[cfg(feature = "notifications-portal")]
    #[test]
    fn portal_backend_reports_kind_and_capabilities() {
        use crate::services::notifications::backend::PortalBackend;
        let backend = PortalBackend::new();
        assert_eq!(backend.kind(), NotificationBackendKind::Portal);
        let caps = backend.capabilities();
        assert!(caps.can_send_immediate_native);
        assert!(caps.requires_packaged_runtime);
    }
}
