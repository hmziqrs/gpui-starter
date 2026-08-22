//! Sanitization helpers for redacting sensitive data in error messages.
//!
//! Provides lightweight pattern-matching (no `regex` crate dependency) for
//! redacting connection strings, tokens, file paths, emails, and hex keys.

/// Maximum length for sanitized error messages (512 characters).
pub const SANITIZE_MAX_LEN: usize = 512;

/// Redact known sensitive patterns from a message string and truncate to
/// [`SANITIZE_MAX_LEN`].
pub(crate) fn sanitize_message(msg: &str) -> String {
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
            p if p.contains("postgres")
                || p.contains("mysql")
                || p.contains("mongodb")
                || p.contains("redis") =>
            {
                redact_url_schemes(
                    self,
                    &["postgres", "mysql", "mongodb", "redis"],
                    replacement,
                )
            }
            // Bearer/token patterns.
            p if p.contains("bearer") || p.contains("token") => {
                redact_tokens(self, replacement)
            }
            // Filesystem paths.
            p if p.contains("/home/")
                || p.contains("/Users/")
                || p.contains("/etc/")
                || p.contains("/var/") =>
            {
                redact_paths(
                    self,
                    &["/home/", "/Users/", "/etc/", "/var/"],
                    replacement,
                )
            }
            // Email addresses.
            p if p.contains("@") && p.contains(".") => {
                redact_emails(self, replacement)
            }
            // Long hex sequences.
            p if p.contains("0-9a-f") => redact_hex(self, replacement),
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
        if lower[i..].starts_with("token=") || lower[i..].starts_with("token:") {
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
            while i < len && chars[i].is_ascii_hexdigit() && !chars[i].is_ascii_whitespace() {
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
