//! Detached process spawning for Unix systems.
//!
//! Provides a [`DetachedProcess`] builder that spawns child processes in their
//! own session (via `setsid()` in `pre_exec`) with null stdio, so the child
//! detaches from the controlling terminal and survives when the parent exits.
//!
//! This is cfg(unix)-gated; it is excluded on macOS-primary builds only when
//! that build is not Unix. (macOS is itself Unix, so this compiles on Linux
//! and macOS alike; it is excluded on Windows.)

#![cfg(unix)]

use std::ffi::OsStr;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use crate::errors::AppError;
use crate::platform::process::session_env;

/// Tracing target for this module.
const LOG: &str = "gpui_starter::detached_process";

/// Builder for creating detached processes.
///
/// A detached process runs in its own session (via `setsid()`) and survives
/// when the parent process exits. All stdio (stdin/stdout/stderr) is
/// redirected to `/dev/null`.
///
/// # Example
/// ```ignore
/// use gpui_starter::platform::process::detached::DetachedProcess;
///
/// DetachedProcess::new("firefox")
///     .arg("https://example.com")
///     .with_session_env()
///     .spawn()?;
/// ```
pub struct DetachedProcess {
    command: Command,
    use_session_env: bool,
    shell_command: Option<String>,
}

impl DetachedProcess {
    /// Create a new detached process builder for the given program.
    pub fn new<S: AsRef<OsStr>>(program: S) -> Self {
        Self {
            command: Command::new(program),
            use_session_env: false,
            shell_command: None,
        }
    }

    /// Create a detached process that runs a shell command.
    ///
    /// The command will be executed via `sh -c "<command>"`.
    pub fn shell<S: Into<String>>(command: S) -> Self {
        let cmd = command.into();
        Self {
            command: Command::new("sh"),
            use_session_env: false,
            shell_command: Some(cmd),
        }
    }

    /// Add a single argument to the process.
    pub fn arg<S: AsRef<OsStr>>(mut self, arg: S) -> Self {
        self.command.arg(arg);
        self
    }

    /// Add multiple arguments to the process.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command.args(args);
        self
    }

    /// Use the captured systemd user session environment for the spawned process.
    ///
    /// When set, [`spawn`](Self::spawn) will clear the inherited environment and
    /// replace it with the session environment captured from
    /// `systemctl --user show-environment`. This is required for detached
    /// children to inherit variables like `DISPLAY`, `WAYLAND_DISPLAY`,
    /// `XDG_CURRENT_DESKTOP`, and theming keys that are normally injected by
    /// the session manager rather than the launching process.
    ///
    /// If the session environment could not be captured, `spawn` logs a warning
    /// and falls back to the parent's inherited environment rather than failing.
    pub fn with_session_env(mut self) -> Self {
        self.use_session_env = true;
        self
    }

    /// Spawn the detached process.
    ///
    /// The spawned process:
    /// - Runs in a new session (calls `setsid()` in `pre_exec`)
    /// - Has stdin/stdout/stderr redirected to `/dev/null`
    /// - Survives when the parent process exits
    ///
    /// # Errors
    /// Returns [`AppError::Io`] if the underlying `spawn()` fails.
    ///
    /// # Safety of the underlying call
    /// This function uses `pre_exec` to call `libc::setsid()`, which is
    /// async-signal-safe and therefore safe to call from the child after
    /// `fork()` and before `exec()`.
    pub fn spawn(mut self) -> Result<(), AppError> {
        // Handle shell commands: sh -c "<cmd>"
        if let Some(cmd) = &self.shell_command {
            self.command.args(["-c", cmd]);
        }

        // Set up environment. If session env capture fails, degrade gracefully
        // by leaving the inherited environment in place.
        if self.use_session_env {
            match session_env::get_session_environment() {
                Ok(env) => {
                    self.command.env_clear();
                    self.command.envs(env.iter());
                }
                Err(err) => {
                    tracing::warn!(
                        target: LOG,
                        error = %err,
                        "failed to capture session environment; \
                         falling back to inherited environment for detached child"
                    );
                }
            }
        }

        // Redirect stdio to null so the child detaches from the controlling terminal.
        self.command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // SAFETY: setsid() is async-signal-safe and creates a new session,
        // detaching the child from the parent's process group so it survives
        // when the parent (e.g. the launcher daemon) exits. We ignore the
        // return value: failure here (e.g. already a process-group leader) is
        // non-fatal for the common detach case.
        unsafe {
            self.command.pre_exec(|| {
                let _ = libc::setsid();
                Ok(())
            });
        }

        self.command.spawn()?;
        tracing::debug!(target: LOG, "spawned detached child process");
        Ok(())
    }
}

/// Launch an application from a whitespace-separated exec string.
///
/// The first token is the program; the remaining tokens are its arguments.
/// An empty or whitespace-only string returns an [`AppError`].
pub fn launch_exec(exec: &str) -> Result<(), AppError> {
    let parts: Vec<&str> = exec.split_whitespace().collect();
    if parts.is_empty() {
        return Err(AppError::Io(std::io::Error::other(
            "cannot launch detached process from empty command string",
        )));
    }

    let program = parts[0];
    let args = &parts[1..];

    DetachedProcess::new(program)
        .args(args.iter().copied())
        .with_session_env()
        .spawn()
}

/// Open a URL using the system default handler (`xdg-open`).
pub fn open_url(url: &str) -> Result<(), AppError> {
    DetachedProcess::new("xdg-open").arg(url).spawn()
}

/// Execute a shell command in a detached process (`sh -c "<command>"`).
pub fn run_shell_command(command: &str) -> Result<(), AppError> {
    DetachedProcess::shell(command).spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_exec_empty_is_error() {
        let result = launch_exec("");
        assert!(result.is_err());
    }

    #[test]
    fn launch_exec_whitespace_only_is_error() {
        let result = launch_exec("   ");
        assert!(result.is_err());
    }

    #[test]
    fn builder_methods_chain() {
        // Ensures the builder API compiles and chains without consuming incorrectly.
        let b = DetachedProcess::new("echo")
            .arg("hello")
            .args(["world"])
            .with_session_env();
        let _ = b;
    }
}
