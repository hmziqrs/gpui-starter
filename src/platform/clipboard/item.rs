//! Clipboard item data structures.
//!
//! Generic clipboard content type used by the history store and the
//! copy/monitor helpers. Kept free of any launcher-specific coupling so it
//! can be reused by any gpui-starter derived app.

use std::time::SystemTime;

/// The content type of a single clipboard entry.
///
/// Only the two universally portable representations are modelled here:
/// plain text and RGBA image data. Rich-text / file-path variants from the
/// upstream reference were intentionally trimmed to keep the boilerplate
/// surface minimal; they can be added later without breaking callers.
#[derive(Clone, Debug)]
pub enum ClipboardContent {
    /// Plain UTF-8 text.
    Text(String),
    /// Raw RGBA pixel data together with its dimensions.
    Image {
        width: usize,
        height: usize,
        rgba_bytes: Vec<u8>,
    },
}

impl PartialEq for ClipboardContent {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(a), Self::Text(b)) => a == b,
            (
                Self::Image {
                    width: aw,
                    height: ah,
                    rgba_bytes: ab,
                },
                Self::Image {
                    width: bw,
                    height: bh,
                    rgba_bytes: bb,
                },
            ) => aw == bw && ah == bh && ab == bb,
            _ => false,
        }
    }
}

impl Eq for ClipboardContent {}

/// A single clipboard history entry: its content plus the moment it was
/// observed.
#[derive(Clone, Debug)]
pub struct ClipboardItem {
    pub content: ClipboardContent,
    pub timestamp: SystemTime,
}

impl ClipboardItem {
    /// Create a new entry stamped with the current time.
    pub fn new(content: ClipboardContent) -> Self {
        Self {
            content,
            timestamp: SystemTime::now(),
        }
    }

    /// Short, single-line preview suitable for list rendering.
    ///
    /// Never panics: empty text collapses to an empty string and images
    /// yield a stable placeholder.
    pub fn preview(&self) -> String {
        const MAX_LENGTH: usize = 30;
        match &self.content {
            ClipboardContent::Text(text) => {
                let first_line = text.lines().next().unwrap_or("");
                truncate_preview_line(first_line, MAX_LENGTH)
            }
            ClipboardContent::Image { width, height, .. } => {
                format!("[Image {width}x{height}]")
            }
        }
    }
}

/// Truncate a preview line at a character boundary (grapheme-cluster safe
/// enough for typical clipboard snippets) without splitting multi-byte
/// sequences, appending an ellipsis when truncated.
fn truncate_preview_line(line: &str, max: usize) -> String {
    let truncated: String = line.chars().take(max).collect();
    if truncated.chars().count() < line.chars().count() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_preview_truncates_long_lines() {
        let item = ClipboardItem::new(ClipboardContent::Text(
            "this is a very long line that should be truncated".into(),
        ));
        let preview = item.preview();
        assert!(preview.ends_with("..."));
        assert!(preview.chars().count() <= 30 + 3);
    }

    #[test]
    fn image_preview_includes_dimensions() {
        let item = ClipboardItem::new(ClipboardContent::Image {
            width: 10,
            height: 20,
            rgba_bytes: vec![],
        });
        assert_eq!(item.preview(), "[Image 10x20]");
    }

    #[test]
    fn content_equality_is_value_based() {
        assert_eq!(
            ClipboardContent::Text("a".into()),
            ClipboardContent::Text("a".into())
        );
        assert_ne!(
            ClipboardContent::Text("a".into()),
            ClipboardContent::Text("b".into())
        );
        assert_ne!(
            ClipboardContent::Text("a".into()),
            ClipboardContent::Image {
                width: 1,
                height: 1,
                rgba_bytes: vec![0]
            }
        );
    }
}
