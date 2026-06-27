//! Self-reload (exec) support for Unix platforms.
//!
//! Provides a cooperative, non-panicking mechanism for restarting the running
//! application in place. A flag is set via [`request_reload`]; once GPUI has
//! shut down cleanly, [`exec_reload`] may be invoked from a post-run hook to
//! replace the current process image with a fresh launch of the same
//! executable and the original command-line arguments.
//!
//! This module is gated behind `#[cfg(unix)]` because it relies on
//! [`std::os::unix::process::CommandExt::exec`], which is unavailable on
//! Windows. macOS is the primary target; the cfg is therefore `unix` rather
//! than `target_os = "linux"` so the macos-primary build benefits too.

#![cfg(unix)]

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use tracing::{error, info};

const LOG: &str = "gpui_starter::reload";

/// Cooperative flag signalling that the app should re-exec after shutdown.
///
/// Set by [`request_reload`] (typically from a `Restart` action handler) and
/// polled by the host after the GPUI run loop has terminated.
static RELOAD_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Request that the application re-launch itself once it has shut down.
///
/// This only sets the flag; the actual [`exec_reload`] must be performed after
/// GPUI has fully torn down (e.g. from a post-run hook in `main`). The caller
/// is responsible for initiating application shutdown afterwards.
pub fn request_reload() {
    RELOAD_REQUESTED.store(true, Ordering::SeqCst);
    info!(target: LOG, "reload requested");
}

/// Returns `true` if [`request_reload`] has been called since the process
/// started (or since the flag was last cleared).
pub fn is_reload_requested() -> bool {
    RELOAD_REQUESTED.load(Ordering::SeqCst)
}

/// Replace the current process image with a fresh launch of this executable,
/// preserving the original command-line arguments.
///
/// # When to call
///
/// This MUST be called only AFTER GPUI has cleanly shut down and the
/// single-instance lock (if any) has been released — otherwise the re-launched
/// process will see itself as a duplicate and refuse to start. The integration
/// host is expected to guard this with [`is_reload_requested`].
///
/// # Failure mode
///
/// On success this function never returns: the process image is replaced. On
/// failure it returns an error describing why `exec` could not be performed;
/// it never panics.
pub fn exec_reload() -> Result<()> {
    use std::os::unix::process::CommandExt;

    info!(target: LOG, "executing in-place reload");

    let exe = std::env::current_exe().context("failed to resolve current executable path")?;

    // Re-launch with the original argv. Skip argv[0] (the program name) since
    // Command::new already supplies argv[0] from `exe`.
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(std::env::args().skip(1));

    let err = cmd.exec();

    // exec() returns the io::Error only when it fails — success never returns.
    error!(target: LOG, error = %err, "exec failed");
    Err(anyhow::anyhow!("failed to exec reload: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_roundtrip() {
        // Reset to a known state; tests run concurrently on the same static,
        // but this process does not actually exec so a benign toggle is fine.
        RELOAD_REQUESTED.store(false, Ordering::SeqCst);
        assert!(!is_reload_requested());
        request_reload();
        assert!(is_reload_requested());
        RELOAD_REQUESTED.store(false, Ordering::SeqCst);
    }
}
