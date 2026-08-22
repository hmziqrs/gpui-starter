#![allow(dead_code)]

use std::{io, path::PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppErrorSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("failed to initialize app paths")]
    PathInitialization,
    #[error("failed to read app state from {path}: {details}")]
    StateRead { path: PathBuf, details: String },
    #[error("failed to parse app state from {path}: {details}")]
    StateParse { path: PathBuf, details: String },
    #[error("failed to write app state to {path}: {details}")]
    StateWrite { path: PathBuf, details: String },
    #[error("invalid deep link `{input}`: {reason}")]
    InvalidDeepLink { input: String, reason: String },
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    /// Error at the IPC (inter-process) boundary: socket connect/write
    /// failures, malformed forwarded messages, or decoding errors.
    /// Serializable so it can ride the typed ForwardedResponse error field.
    #[error("ipc error: {message}")]
    Ipc { message: String },
}

impl AppError {
    /// Construct an `InvalidDeepLink` error from any string-like input/reason.
    pub fn invalid_deep_link(
        input: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::InvalidDeepLink {
            input: input.into(),
            reason: reason.into(),
        }
    }

    pub fn severity(&self) -> AppErrorSeverity {
        match self {
            Self::InvalidDeepLink { .. } | Self::StateParse { .. } | Self::Ipc { .. } => {
                AppErrorSeverity::Warning
            }
            Self::PathInitialization
            | Self::StateRead { .. }
            | Self::StateWrite { .. }
            | Self::Io(_) => AppErrorSeverity::Error,
        }
    }
}
