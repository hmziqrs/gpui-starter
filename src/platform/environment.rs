//! Linux / sandbox environment detection.

/// Whether the process is running inside a Flatpak, Snap, or similar sandbox.
///
/// Reimplemented here (rather than reusing `ashpd::is_sandboxed()`) so it is
/// available even when the `notifications-portal` feature is OFF (the default):
/// native packaging (`.deb` / Nix / tarball / AppImage) never enables it. The
/// decision to use the portal backend hinges on this.
pub fn is_sandboxed() -> bool {
    std::env::var_os("FLATPAK_ID").is_some()
        || std::path::Path::new("/.flatpak-info").exists()
        || std::env::var_os("SNAP").is_some()
}
