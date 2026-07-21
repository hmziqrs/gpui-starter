//! Systemd user session environment capture.
//!
//! Captures the environment exported by `systemctl --user show-environment`
//! (one `KEY=VALUE` per line) so it can be forwarded to detached child
//! processes that otherwise lose access to session-scoped variables such as
//! `DISPLAY`, `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`, and `XDG_CURRENT_DESKTOP`.
//!
//! This module is `cfg(unix)`-gated. It does not depend on a running systemd
//! session at compile time; failures to invoke `systemctl` are reported via
//! [`AppError`] and logged, never panicked.

#![cfg(unix)]

use std::collections::HashMap;
use std::process::Command;

use crate::errors::AppError;

/// Tracing target for this module.
const LOG: &str = "gpui_starter::session_env";

/// Errors that occur while capturing the session environment.
///
/// Re-used [`AppError`] variants cover the io and parse cases; the relevant
/// detail is included in the variant's message string.
type SessionEnvResult<T> = Result<T, AppError>;

/// Capture the systemd user session environment.
///
/// Runs `systemctl --user show-environment` and parses the output into a map
/// of `KEY` -> `VALUE`. Each line is expected to be of the form `KEY=VALUE`.
/// Lines without an `=` separator are skipped (logged at debug level) rather
/// than failing the whole capture.
///
/// # Errors
/// - [`AppError::Io`] if `systemctl` cannot be spawned or its output cannot be
///   read as UTF-8.
/// - [`AppError::Io`] if `systemctl` exits with a non-zero status (surfaced as
///   an io error carrying the exit code).
pub fn get_session_environment() -> SessionEnvResult<HashMap<String, String>> {
    let output = Command::new("systemctl")
        .args(["--user", "show-environment"])
        .output()?;

    if !output.status.success() {
        let code = output.status.code();
        tracing::warn!(
            target: LOG,
            ?code,
            "systemctl --user show-environment exited non-zero"
        );
        return Err(AppError::Io(std::io::Error::other(format!(
            "systemctl --user show-environment failed (exit code: {code:?})"
        ))));
    }

    let stdout = String::from_utf8(output.stdout).map_err(|err| {
        AppError::Io(std::io::Error::other(format!(
            "session environment output was not valid UTF-8: {err}"
        )))
    })?;

    Ok(parse_environment(&stdout))
}

/// Parse `KEY=VALUE` lines into a map.
///
/// Blank lines, lines lacking an `=`, and lines with an empty key are skipped.
/// Leading/trailing whitespace is trimmed from both key and value.
fn parse_environment(raw: &str) -> HashMap<String, String> {
    let mut env = HashMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            tracing::debug!(
                target: LOG,
                line,
                "skipping session environment line without '=' separator"
            );
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        env.insert(key.to_string(), value.trim().to_string());
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_key_value_lines() {
        let raw = "FOO=bar\nBAZ=qux\n";
        let env = parse_environment(raw);
        assert_eq!(env.get("FOO").map(String::as_str), Some("bar"));
        assert_eq!(env.get("BAZ").map(String::as_str), Some("qux"));
    }

    #[test]
    fn parse_skips_blank_and_separatorless_lines() {
        let raw = "\nDISPLAY=:0\nGARBAGE\n=missingkey\n";
        let env = parse_environment(raw);
        assert_eq!(env.len(), 1);
        assert_eq!(env.get("DISPLAY").map(String::as_str), Some(":0"));
    }

    #[test]
    fn parse_preserves_values_containing_equals() {
        let raw = "PATH=/usr/bin:/bin=weird\n";
        let env = parse_environment(raw);
        assert_eq!(
            env.get("PATH").map(String::as_str),
            Some("/usr/bin:/bin=weird")
        );
    }
}
