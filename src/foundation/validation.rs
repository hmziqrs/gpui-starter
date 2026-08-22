#![allow(dead_code)]

use std::path::{Path, PathBuf};

use thiserror::Error;
use url::Url;

use crate::errors::AppError;
use crate::routes::{APP_URL_SCHEME, VALID_HOSTS};

// ---------------------------------------------------------------------------
// Deep-link URL validation
// ---------------------------------------------------------------------------

/// Validate that a deep-link URL uses the expected scheme (`gpui-starter://`)
/// and reject unexpected hosts.
pub fn validate_deep_link_url(url: &str) -> Result<Url, AppError> {
    let parsed =
        Url::parse(url).map_err(|err| AppError::invalid_deep_link(url, err.to_string()))?;

    if parsed.scheme() != APP_URL_SCHEME {
        return Err(AppError::invalid_deep_link(
            url,
            format!("unsupported scheme `{}`", parsed.scheme()),
        ));
    }

    let host = parsed.host_str().unwrap_or_default();
    if !VALID_HOSTS.contains(&host) {
        return Err(AppError::invalid_deep_link(
            url,
            format!("unexpected host `{host}`"),
        ));
    }

    Ok(parsed)
}

// ---------------------------------------------------------------------------
// File-path validation
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("path traversal detected in `{input}`")]
    PathTraversal { input: String },

    #[error("path `{input}` escapes allowed directory")]
    EscapesAllowedDir { input: String },

    #[error("path does not exist: `{input}`")]
    NotFound { input: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Canonicalize a path and verify it contains no `..` traversal components
/// and stays within one of the given `allowed_dirs`.
pub fn validate_file_path(
    path: &str,
    allowed_dirs: &[PathBuf],
) -> Result<PathBuf, ValidationError> {
    let raw = Path::new(path);

    // Reject any component that is a parent-directory marker.
    for component in raw.components() {
        if component == std::path::Component::ParentDir {
            return Err(ValidationError::PathTraversal {
                input: path.to_string(),
            });
        }
    }

    // Resolve to a canonical absolute path (file must exist).
    let canonical = raw.canonicalize().map_err(|_| ValidationError::NotFound {
        input: path.to_string(),
    })?;

    // Verify the canonical path starts with at least one allowed dir.
    let permitted = allowed_dirs.iter().any(|dir| {
        let Ok(canonical_dir) = dir.canonicalize() else {
            return false;
        };
        canonical.starts_with(&canonical_dir)
    });

    if !permitted {
        return Err(ValidationError::EscapesAllowedDir {
            input: path.to_string(),
        });
    }

    Ok(canonical)
}

// ---------------------------------------------------------------------------
// String sanitization
// ---------------------------------------------------------------------------

/// Maximum length for a sanitized string.
pub const MAX_SANITIZED_LENGTH: usize = 4096;

/// Strip ASCII control characters (except newline and tab), trim whitespace,
/// and enforce a length limit.
pub fn sanitize_string(input: &str) -> String {
    let mut out: String = input
        .chars()
        .filter(|c| {
            if c.is_control() {
                // Keep newline and tab — they are legitimate whitespace.
                *c == '\n' || *c == '\t'
            } else {
                true
            }
        })
        .collect();

    out.truncate(MAX_SANITIZED_LENGTH);
    out.trim().to_string()
}

// ---------------------------------------------------------------------------
// Notification ID validation
// ---------------------------------------------------------------------------

/// Return `true` when `id` is non-empty and consists solely of ASCII
/// alphanumeric characters and hyphens.
pub fn validate_notification_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "validation.test.rs"]
mod validation_test;
