//! Clipboard write helpers.
//!
//! Thin, fallible wrappers around [`arboard::Clipboard`] for setting text
//! and image content. Failures are surfaced as [`ClipboardError`] (a
//! dedicated error variant) rather than panicking, matching the
//! boilerplate's "return Result" convention.

use std::fmt;

use arboard::{Clipboard, ImageData};

use super::item::ClipboardContent;

const LOG: &str = "gpui_starter::clipboard::copy";

/// Errors that can occur while writing to the system clipboard.
#[derive(Debug)]
pub enum ClipboardError {
    /// The platform clipboard could not be opened.
    AccessFailed(String),
    /// The write (set text/image) was rejected by the clipboard.
    WriteFailed(String),
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccessFailed(details) => {
                write!(f, "clipboard access failed: {details}")
            }
            Self::WriteFailed(details) => {
                write!(f, "clipboard write failed: {details}")
            }
        }
    }
}

impl std::error::Error for ClipboardError {}

impl From<arboard::Error> for ClipboardError {
    fn from(err: arboard::Error) -> Self {
        Self::AccessFailed(err.to_string())
    }
}

/// Write plain text to the system clipboard.
pub fn set_text(text: &str) -> Result<(), ClipboardError> {
    let mut clipboard =
        Clipboard::new().map_err(|err| ClipboardError::AccessFailed(err.to_string()))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|err| ClipboardError::WriteFailed(err.to_string()))?;
    tracing::debug!(target: LOG, len = text.len(), "wrote text to clipboard");
    Ok(())
}

/// Write an RGBA image to the system clipboard.
///
/// `rgba_bytes` must be `width * height * 4` bytes long; callers are
/// responsible for ensuring this invariant (an inconsistency surfaces as a
/// [`ClipboardError::WriteFailed`]).
pub fn set_image(width: usize, height: usize, rgba_bytes: &[u8]) -> Result<(), ClipboardError> {
    let mut clipboard =
        Clipboard::new().map_err(|err| ClipboardError::AccessFailed(err.to_string()))?;
    let image_data = ImageData {
        width,
        height,
        bytes: std::borrow::Cow::Borrowed(rgba_bytes),
    };
    clipboard
        .set_image(image_data)
        .map_err(|err| ClipboardError::WriteFailed(err.to_string()))?;
    tracing::debug!(
        target: LOG,
        width, height, bytes = rgba_bytes.len(),
        "wrote image to clipboard"
    );
    Ok(())
}

/// Write arbitrary [`ClipboardContent`] to the clipboard.
///
/// Convenience dispatcher used by both the copy hot-path and (feature-gated)
/// re-publish of a history entry.
pub fn set_content(content: &ClipboardContent) -> Result<(), ClipboardError> {
    match content {
        ClipboardContent::Text(text) => set_text(text),
        ClipboardContent::Image {
            width,
            height,
            rgba_bytes,
        } => set_image(*width, *height, rgba_bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err = ClipboardError::AccessFailed("boom".into());
        assert!(err.to_string().contains("boom"));
        let err = ClipboardError::WriteFailed("nope".into());
        assert!(err.to_string().contains("nope"));
    }

    #[test]
    fn arboard_error_converts() {
        let err = ClipboardError::from(arboard::Error::ContentNotAvailable);
        assert!(matches!(err, ClipboardError::AccessFailed(_)));
    }
}
