use gpui::{App, BorrowAppContext as _, Global, SharedString};

#[derive(Debug, thiserror::Error)]
pub enum SecureStorageError {
    #[error("entry creation failed for '{service}/{key}': {source}")]
    EntryCreation {
        service: String,
        key: String,
        #[source]
        source: keyring::Error,
    },
    #[error("failed to set secret for '{service}/{key}': {source}")]
    SetFailed {
        service: String,
        key: String,
        #[source]
        source: keyring::Error,
    },
    #[error("failed to get secret for '{service}/{key}': {source}")]
    GetFailed {
        service: String,
        key: String,
        #[source]
        source: keyring::Error,
    },
    #[error("failed to delete secret for '{service}/{key}': {source}")]
    DeleteFailed {
        service: String,
        key: String,
        #[source]
        source: keyring::Error,
    },
}

#[derive(Clone, Debug, Default)]
pub struct SecureStorageSnapshot {
    pub available: bool,
    pub last_error: Option<String>,
}

impl Global for SecureStorageSnapshot {}

pub fn initialize(cx: &mut App) {
    let available = keyring::Entry::new("gpui-starter", "availability-check").is_ok();
    let snapshot = SecureStorageSnapshot {
        available,
        last_error: if available {
            None
        } else {
            Some("keyring entry initialization unavailable".to_string())
        },
    };
    cx.set_global(snapshot.clone());
    let status = if snapshot.available {
        crate::capabilities::CapabilityStatus::supported_enabled()
    } else {
        crate::capabilities::CapabilityStatus::error(
            snapshot
                .last_error
                .as_deref()
                .unwrap_or("secure storage unavailable"),
        )
    };
    crate::capabilities::set("secure_storage", status, cx);
}

pub fn snapshot(cx: &App) -> SecureStorageSnapshot {
    cx.try_global::<SecureStorageSnapshot>()
        .cloned()
        .unwrap_or_default()
}

pub fn set_secret(
    service: &str,
    key: &str,
    value: &str,
    cx: &mut App,
) -> Result<(), SecureStorageError> {
    let entry = keyring::Entry::new(service, key).map_err(|err| {
        tracing::error!(target: "gpui_starter::secure_storage", "entry creation failed: {err}");
        SecureStorageError::EntryCreation {
            service: service.to_string(),
            key: key.to_string(),
            source: err,
        }
    })?;
    entry.set_password(value).map_err(|err| {
        tracing::error!(target: "gpui_starter::secure_storage", service, key, "set_password failed: {err}");
        update_last_error(Some(err.to_string()), cx);
        SecureStorageError::SetFailed { service: service.to_string(), key: key.to_string(), source: err }
    })?;
    tracing::info!(target: "gpui_starter::secure_storage", service, key, "secret written");
    update_last_error(None, cx);
    Ok(())
}

pub fn get_secret(
    service: &str,
    key: &str,
    cx: &mut App,
) -> Result<Option<SharedString>, SecureStorageError> {
    let entry = keyring::Entry::new(service, key).map_err(|err| {
        tracing::error!(target: "gpui_starter::secure_storage", "entry creation failed: {err}");
        SecureStorageError::EntryCreation {
            service: service.to_string(),
            key: key.to_string(),
            source: err,
        }
    })?;
    match entry.get_password() {
        Ok(value) => {
            tracing::info!(target: "gpui_starter::secure_storage", service, key, "secret read");
            update_last_error(None, cx);
            Ok(Some(value.into()))
        }
        Err(keyring::Error::NoEntry) => {
            tracing::warn!(target: "gpui_starter::secure_storage", service, key, "no entry found");
            Ok(None)
        }
        Err(err) => {
            tracing::error!(target: "gpui_starter::secure_storage", service, key, "get_password failed: {err}");
            Err(SecureStorageError::GetFailed {
                service: service.to_string(),
                key: key.to_string(),
                source: err,
            })
        }
    }
}

pub fn delete_secret(service: &str, key: &str, cx: &mut App) -> Result<(), SecureStorageError> {
    let entry = keyring::Entry::new(service, key).map_err(|err| {
        tracing::error!(target: "gpui_starter::secure_storage", "entry creation failed: {err}");
        SecureStorageError::EntryCreation {
            service: service.to_string(),
            key: key.to_string(),
            source: err,
        }
    })?;
    entry.delete_credential().map_err(|err| {
        tracing::error!(target: "gpui_starter::secure_storage", service, key, "delete failed: {err}");
        update_last_error(Some(err.to_string()), cx);
        SecureStorageError::DeleteFailed { service: service.to_string(), key: key.to_string(), source: err }
    })?;
    tracing::info!(target: "gpui_starter::secure_storage", service, key, "secret deleted");
    update_last_error(None, cx);
    Ok(())
}

fn update_last_error(last_error: Option<String>, cx: &mut App) {
    cx.update_global::<SecureStorageSnapshot, _>(|state, _cx| {
        state.last_error = last_error;
    });
}
