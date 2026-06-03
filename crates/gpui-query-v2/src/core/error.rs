//! Error types for query operations.
//!
//! [`QueryError`] is the default error type for query resources. It implements
//! [`std::fmt::Display`] and [`std::error::Error`] for ecosystem interop
//! with `?` propagation and `anyhow`.
//!
//! # Security note
//!
//! Error messages passed to [`QueryError`] are stored verbatim and may appear
//! in logs, DevTools diagnostics, and serialized output. Callers **must**
//! sanitize server responses before constructing a `QueryError` to avoid
//! leaking sensitive data (internal paths, credentials, auth tokens, etc.).
//! Use [`QueryError::sanitized`] to redact known sensitive patterns.

use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The kind of error that occurred during a query operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryErrorKind {
    /// The request was cancelled (cooperative cancellation).
    Cancelled,
    /// The server returned an error response.
    Response,
    /// A transport-level error occurred (network, timeout).
    Transport,
    /// An unknown error occurred.
    Unknown,
}

impl std::fmt::Display for QueryErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "cancelled"),
            Self::Response => write!(f, "response error"),
            Self::Transport => write!(f, "transport error"),
            Self::Unknown => write!(f, "unknown error"),
        }
    }
}

/// An error produced by a query or mutation operation.
///
/// Implements [`std::error::Error`] so it can be used with `?` propagation
/// and libraries like `anyhow`.
///
/// The internal message uses `Arc<str>` so cloning is cheap, making this
/// suitable for high-retry scenarios where the same error is stored in
/// multiple locations.
///
/// # Example
///
/// ```
/// use gpui_query_v2::QueryError;
///
/// let err = QueryError::response("not found");
/// assert_eq!(err.to_string(), "response error: not found");
/// ```
///
/// # Security
///
/// Error messages are included in debug/display output and may be serialized.
/// When constructing errors from server responses, sanitize the message first
/// or use [`QueryError::sanitized`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryError {
    kind: QueryErrorKind,
    message: Arc<str>,
}

impl QueryError {
    /// Create a new error with the given kind and message.
    pub fn new(kind: QueryErrorKind, message: impl Into<Arc<str>>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Create a cancellation error.
    pub fn cancelled(message: impl Into<Arc<str>>) -> Self {
        Self::new(QueryErrorKind::Cancelled, message)
    }

    /// Create a response error (server-side).
    pub fn response(message: impl Into<Arc<str>>) -> Self {
        Self::new(QueryErrorKind::Response, message)
    }

    /// Create a transport error (network, timeout).
    pub fn transport(message: impl Into<Arc<str>>) -> Self {
        Self::new(QueryErrorKind::Transport, message)
    }

    /// Create an unknown error.
    pub fn unknown(message: impl Into<Arc<str>>) -> Self {
        Self::new(QueryErrorKind::Unknown, message)
    }

    /// The kind of error.
    pub fn kind(&self) -> QueryErrorKind {
        self.kind
    }

    /// The error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Return a sanitized copy of this error with known sensitive patterns redacted.
    ///
    /// Redacts common patterns such as:
    /// - Database connection strings (`postgres://...`, `mysql://...`)
    /// - Bearer / token headers (`Bearer ...`, `token=...`)
    /// - File paths (`/home/...`, `/Users/...`, `/etc/...`)
    /// - Email-like strings
    /// - Long hex sequences (likely API keys)
    ///
    /// Also truncates the message to [`SANITIZE_MAX_LEN`] bytes.
    pub fn sanitized(&self) -> Self {
        let redacted = sanitize_message(&self.message);
        Self {
            kind: self.kind,
            message: redacted.into(),
        }
    }
}

/// Maximum length for sanitized error messages (512 characters).
pub const SANITIZE_MAX_LEN: usize = 512;

/// Redact known sensitive patterns from a message string and truncate to
/// [`SANITIZE_MAX_LEN`].
fn sanitize_message(msg: &str) -> String {
    use std::borrow::Cow;

    let mut out = Cow::Borrowed(msg);

    // Redact database connection strings.
    out = Cow::Owned(out.replace_regex(
        r"(?i)(postgres|mysql|mongodb|redis)://\S+",
        "[REDACTED_CONNECTION]",
    ));
    // Redact bearer/token patterns.
    out = Cow::Owned(out.replace_regex(
        r"(?i)(bearer\s+|token[=:]\s*)\S+",
        "$1[REDACTED_TOKEN]",
    ));
    // Redact common filesystem paths.
    out = Cow::Owned(out.replace_regex(
        r"(?i)(/home/|/Users/|/etc/|/var/)\S+",
        "[REDACTED_PATH]",
    ));
    // Redact email addresses.
    out = Cow::Owned(out.replace_regex(
        r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b",
        "[REDACTED_EMAIL]",
    ));
    // Redact long hex sequences (likely API keys or secrets).
    out = Cow::Owned(out.replace_regex(
        r"\b[0-9a-fA-F]{16,}\b",
        "[REDACTED_HEX]",
    ));

    let mut s = out.into_owned();
    if s.len() > SANITIZE_MAX_LEN {
        s.truncate(SANITIZE_MAX_LEN);
        s.push_str("...[truncated]");
    }
    s
}

/// Helper trait so we can call `replace_regex` on `String` / `Cow<str>`.
/// Uses a simple approach without pulling in the `regex` crate: manual
/// scan-and-replace for each pattern. This keeps the dependency footprint
/// minimal for a utility that only runs on DevTools / logging paths.
trait Redact {
    fn replace_regex(&self, pattern: &str, replacement: &str) -> String;
}

impl Redact for str {
    fn replace_regex(&self, pattern: &str, replacement: &str) -> String {
        // Lightweight pattern matching without the regex crate.
        // We only handle the specific patterns used by `sanitize_message`.
        match pattern {
            // Database connection strings.
            p if p.contains("postgres") || p.contains("mysql") || p.contains("mongodb") || p.contains("redis") => {
                redact_url_schemes(self, &["postgres", "mysql", "mongodb", "redis"], replacement)
            }
            // Bearer/token patterns.
            p if p.contains("bearer") || p.contains("token") => {
                redact_tokens(self, replacement)
            }
            // Filesystem paths.
            p if p.contains("/home/") || p.contains("/Users/") || p.contains("/etc/") || p.contains("/var/") => {
                redact_paths(self, &["/home/", "/Users/", "/etc/", "/var/"], replacement)
            }
            // Email addresses.
            p if p.contains("@") && p.contains(".") => {
                redact_emails(self, replacement)
            }
            // Long hex sequences.
            p if p.contains("0-9a-f") => {
                redact_hex(self, replacement)
            }
            _ => self.to_string(),
        }
    }
}

/// Redact URL-like connection strings starting with any of `schemes`.
fn redact_url_schemes(text: &str, schemes: &[&str], replacement: &str) -> String {
    let mut result = text.to_string();
    for scheme in schemes {
        // Case-insensitive scan for `scheme://...` (non-whitespace run).
        let lower = result.to_ascii_lowercase();
        let mut offset = 0;
        let mut new = String::with_capacity(result.len());
        while let Some(pos) = lower[offset..].find(&format!("{}://", scheme)) {
            let abs_pos = offset + pos;
            // Find end of the URL (next whitespace or end).
            let end = result[abs_pos..]
                .find(|c: char| c.is_whitespace())
                .map_or(result.len(), |i| abs_pos + i);
            new.push_str(&result[offset..abs_pos]);
            new.push_str(replacement);
            offset = end;
        }
        new.push_str(&result[offset..]);
        result = new;
    }
    result
}

/// Redact bearer/token patterns.
fn redact_tokens(text: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let len = text.len();
    let mut i = 0;

    while i < len {
        // Check for "bearer " prefix.
        if lower[i..].starts_with("bearer ") {
            result.push_str(&text[i..i + 7]); // "bearer "
            i += 7;
            // Consume token until whitespace or end.
            while i < len && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            result.push_str(replacement);
            continue;
        }
        // Check for "token=" or "token:" followed by a value.
        if lower[i..].starts_with("token=")
            || lower[i..].starts_with("token:")
        {
            let prefix_len = 6; // "token=" or "token:"
            result.push_str(&text[i..i + prefix_len]);
            i += prefix_len;
            while i < len && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            result.push_str(replacement);
            continue;
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

/// Redact filesystem paths starting with any of `prefixes`.
fn redact_paths(text: &str, prefixes: &[&str], replacement: &str) -> String {
    let mut result = text.to_string();
    for prefix in prefixes {
        let mut offset = 0;
        let mut new = String::with_capacity(result.len());
        let lower = result.to_ascii_lowercase();
        while let Some(pos) = lower[offset..].find(prefix) {
            let abs_pos = offset + pos;
            let end = result[abs_pos..]
                .find(|c: char| c.is_whitespace())
                .map_or(result.len(), |i| abs_pos + i);
            new.push_str(&result[offset..abs_pos]);
            new.push_str(replacement);
            offset = end;
        }
        new.push_str(&result[offset..]);
        result = new;
    }
    result
}

/// Redact email addresses (simple heuristic: word@word.word).
fn redact_emails(text: &str, replacement: &str) -> String {
    let mut result = String::new();
    let mut i = 0;
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    while i < len {
        // Try to match an email starting at position i.
        if let Some(email_end) = try_match_email(&chars, i) {
            result.push_str(replacement);
            i = email_end;
            continue;
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

/// Try to match an email at position `start` in `chars`. Returns end index if matched.
fn try_match_email(chars: &[char], start: usize) -> Option<usize> {
    let len = chars.len();
    if start >= len {
        return None;
    }

    // Local part: alphanumeric + ._%+-
    let mut i = start;
    if i >= len || !chars[i].is_alphanumeric() {
        return None;
    }
    while i < len && (chars[i].is_alphanumeric() || ".%+-".contains(chars[i])) {
        i += 1;
    }
    if i >= len || chars[i] != '@' {
        return None;
    }
    i += 1; // skip '@'

    // Domain: alphanumeric + .-
    if i >= len || !chars[i].is_alphanumeric() {
        return None;
    }
    while i < len && (chars[i].is_alphanumeric() || ".-".contains(chars[i])) {
        i += 1;
    }

    // Must end with a dot followed by 2+ alpha chars (TLD).
    let domain_end = i;
    if domain_end > start + 2 {
        // Walk backwards to find last dot in the matched portion.
        let mut last_dot = None;
        for j in (start..domain_end).rev() {
            if chars[j] == '.' {
                last_dot = Some(j);
                break;
            }
        }
        if let Some(dot_pos) = last_dot {
            let tld_len = domain_end - dot_pos - 1;
            if tld_len >= 2 && chars[dot_pos + 1..domain_end].iter().all(|c| c.is_alphabetic()) {
                return Some(domain_end);
            }
        }
    }
    None
}

/// Redact long hex sequences (16+ hex chars).
fn redact_hex(text: &str, replacement: &str) -> String {
    let mut result = String::new();
    let mut i = 0;
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    while i < len {
        if chars[i].is_ascii_hexdigit()
            && (chars[i] as u8) >= b'0'
            && !chars[i].is_ascii_whitespace()
        {
            // Count consecutive hex chars.
            let start = i;
            while i < len
                && chars[i].is_ascii_hexdigit()
                && !chars[i].is_ascii_whitespace()
            {
                i += 1;
            }
            if i - start >= 16 {
                result.push_str(replacement);
            } else {
                for c in &chars[start..i] {
                    result.push(*c);
                }
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

// --- Serde: serialize/deserialize `Arc<str>` as a plain string ---

impl Serialize for QueryError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Helper {
            kind: QueryErrorKind,
            message: String,
        }
        Helper {
            kind: self.kind,
            message: self.message.to_string(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for QueryError {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Helper {
            kind: QueryErrorKind,
            message: String,
        }
        let h = Helper::deserialize(deserializer)?;
        Ok(Self {
            kind: h.kind,
            message: h.message.into(),
        })
    }
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for QueryError {}

impl AsRef<str> for QueryError {
    fn as_ref(&self) -> &str {
        self.message()
    }
}

impl From<String> for QueryError {
    fn from(value: String) -> Self {
        Self::unknown(value)
    }
}

impl From<&str> for QueryError {
    fn from(value: &str) -> Self {
        Self::unknown(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitized_redacts_connection_strings() {
        let err = QueryError::transport("connection to postgres://user:pass@host/db failed");
        let clean = err.sanitized();
        assert!(!clean.message().contains("user:pass@host"));
        assert!(clean.message().contains("[REDACTED_CONNECTION]"));
    }

    #[test]
    fn sanitized_redacts_bearer_tokens() {
        let err = QueryError::response("auth failed: bearer abc123token");
        let clean = err.sanitized();
        assert!(!clean.message().contains("abc123token"));
        assert!(clean.message().contains("[REDACTED_TOKEN]"));
    }

    #[test]
    fn sanitized_redacts_file_paths() {
        let err = QueryError::unknown("error reading /home/user/secret.key");
        let clean = err.sanitized();
        assert!(!clean.message().contains("/home/user/secret.key"));
        assert!(clean.message().contains("[REDACTED_PATH]"));
    }

    #[test]
    fn sanitized_redacts_emails() {
        let err = QueryError::response("user admin@example.com not found");
        let clean = err.sanitized();
        assert!(!clean.message().contains("admin@example.com"));
        assert!(clean.message().contains("[REDACTED_EMAIL]"));
    }

    #[test]
    fn sanitized_redacts_long_hex() {
        let err = QueryError::response("key a1b2c3d4e5f6a1b2c3d4e5f6a1b2 rejected");
        let clean = err.sanitized();
        assert!(!clean.message().contains("a1b2c3d4e5f6a1b2c3d4e5f6a1b2"));
        assert!(clean.message().contains("[REDACTED_HEX]"));
    }

    #[test]
    fn sanitized_truncates_long_messages() {
        let long_msg = "x".repeat(600);
        let err = QueryError::unknown(&*long_msg);
        let clean = err.sanitized();
        assert!(clean.message().len() <= SANITIZE_MAX_LEN + "...[truncated]".len());
        assert!(clean.message().ends_with("...[truncated]"));
    }

    #[test]
    fn sanitized_preserves_kind() {
        let err = QueryError::cancelled("aborted");
        assert_eq!(err.sanitized().kind(), QueryErrorKind::Cancelled);
    }

    #[test]
    fn clone_is_cheap() {
        let err = QueryError::response("test error");
        let err2 = err.clone();
        assert!(Arc::ptr_eq(&err.message, &err2.message));
    }
}
