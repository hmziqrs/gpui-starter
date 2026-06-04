---
title: Auto-Updater
description: Signed auto-updates with Ed25519 verification, GitHub Releases integration, and user prompts.
---

The auto-updater service (`src/services/updater.rs`) checks for new versions, downloads them, verifies Ed25519 signatures and macOS codesign, and swaps the binary on next launch. It runs on a 4-hour periodic timer and dispatches notifications when updates are available.

This page covers the update lifecycle, manifest format, signature verification, configuration, and the release pipeline. For the notification system that delivers update prompts, see [Notifications](/docs/notifications/). For the CI workflow, see [CI/CD for Rust desktop apps](/blog/rust-desktop-ci-cd-guide/).

## Update lifecycle

The updater follows a state machine with seven states:

```
Idle -> Checking -> Available -> Downloading -> Downloaded -> ReadyToInstall
                                    |                           |
                                    v                           v
                               (on failure)              (next launch swap)
                                    |
                                    v
                                  Error
```

Each state is represented by `UpdateStatus`:

```rust
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Available { version: String, notes: String },
    Downloading { progress: u32 },  // 0-100
    Downloaded { version: String, path: String },
    ReadyToInstall,
    Error(String),
}
```

The current state is stored as a GPUI global (`UpdateSnapshot`). Any component can read it with `snapshot(cx)`. The status bar in `src/shell/status_bar.rs` uses this to display update indicators.

### When checks happen

The updater checks on three triggers:

1. **Startup**: 5 seconds after `initialize(cx)` is called during app init.
2. **Periodic**: Every 4 hours, but only if the current status is `Idle`, `UpToDate`, or `Error`. Checks are skipped while a download is in progress.
3. **Manual**: Dispatch the `CheckForUpdates` action from a button or menu item.

Before each check, the updater verifies network connectivity. If the app is offline, the check fails immediately and retry backoff kicks in.

## Initialization

Call `initialize` during app startup, after the Tokio runtime global is installed:

```rust
services::updater::initialize(cx);
```

This installs the `UpdateSnapshot` global, registers the `CheckForUpdates` action handler, and schedules both the startup and periodic check loops.

## Checking for updates

You can trigger a check programmatically:

```rust
use crate::services::updater;

// From an action handler or button callback:
updater::check_for_updates(cx);

// Or dispatch the action:
cx.dispatch_action(Box::new(updater::CheckForUpdates));
```

The check is guarded: if the status is already `Checking` or `Downloading`, the call is a no-op. On a successful check, the manifest version is compared against the current app version using semver. If newer, the status moves to `Available` and a notification is dispatched.

## Downloading and installing

Once an update is available, call `download_update` to start the download:

```rust
updater::download_update(cx);
```

The download streams the asset to a temp directory with progress reporting (updates every 10% change). After download, three verification steps run:

1. **Ed25519 signature**: Verifies the manifest's base64 signature against the SHA-256 hash of the downloaded file using the hardcoded public key. Failure deletes the download.
2. **macOS codesign**: Runs `codesign --verify --deep --strict` on macOS. Failure rejects the download.
3. **Size audit**: The downloaded byte count is logged.

After verification, call `apply_update(cx)` to schedule the swap. This writes a pending swap marker to the app data directory. On next launch, `check_pending_swap(cx)` reads the marker and replaces the binary or `.app` bundle.

### How the swap works

On macOS, the updater detects `.app` bundles and replaces the entire bundle. For standalone binaries (or on Linux/Windows), the new binary is renamed over the current executable. The swap runs at startup before the event loop to avoid file locks.

## Manifest format

The updater fetches a JSON manifest from a configurable URL:

```json
{
  "version": "0.3.0",
  "release_notes": "Fixed crash on startup, added dark theme variants.",
  "platforms": {
    "macos-aarch64": {
      "url": "https://github.com/myorg/gpui-app/releases/download/v0.3.0/GPUI-Starter-0.3.0.dmg",
      "signature": "base64-encoded-ed25519-signature",
      "size": 84926864
    },
    "linux-x86_64": {
      "url": "https://example.com/app-linux.tar.gz",
      "signature": "",
      "size": 42318848
    }
  }
}
```

Platform keys use the pattern `{os}-{arch}`: `macos-aarch64`, `linux-x86_64`, `windows-x86_64`. The `release_notes`, `platforms`, and `signature` fields default to empty if omitted. An empty signature skips Ed25519 verification with a logged warning.

## Configuration

### Manifest URL

Set the manifest URL at compile time via the `GPUI_UPDATE_MANIFEST_URL` environment variable:

```bash
GPUI_UPDATE_MANIFEST_URL=https://releases.myapp.com/manifest.json cargo build --release
```

If not set, it defaults to `https://releases.example.com/manifest.json`. Set this env var in your build pipeline to point to your actual manifest.

### Update channels

Channels are stored in the persisted app config and read at startup:

```rust
// Read current channel
let snap = updater::snapshot(cx);
println!("Channel: {}", snap.update_channel);

// Change channel at runtime
updater::set_channel("beta", cx);
```

The default channel is `"stable"`. The channel value is persisted in the config file and survives restarts. Your manifest server can use the channel to serve different versions to different user segments.

### Retry and backoff

Failed checks and downloads retry up to 3 times with exponential backoff. The base delay is 30 seconds, so retries happen at 30s, 60s, and 120s. After 3 failures, the status moves to `Error` permanently and a notification is dispatched. Successful checks and downloads reset the retry counter.

### Timing constants

| Constant | Value | Purpose |
|----------|-------|---------|
| `STARTUP_CHECK_DELAY_SECS` | 5 | Delay before first check after launch |
| `PERIODIC_CHECK_INTERVAL_SECS` | 14400 (4h) | Time between automatic checks |
| `MAX_UPDATE_RETRIES` | 3 | Max retry attempts for check or download |
| `RETRY_BASE_DELAY_SECS` | 30 | Base delay for exponential backoff |

## Ed25519 signing

The updater verifies that downloaded assets were signed by you and not tampered with. The manifest includes a base64-encoded Ed25519 signature for each platform asset. The updater computes the SHA-256 hash of the downloaded file and verifies the signature against that hash using the embedded public key (`src/services/updater_public_key.bin`, a raw 32-byte file compiled in via `include_bytes!`).

### Generating keys

Generate a key pair with Python's `cryptography` package:

```bash
python3 -c "
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
key = Ed25519PrivateKey.generate()
pub = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
open('updater_public_key.bin', 'wb').write(pub)
import base64; print(base64.b64encode(key.private_bytes_raw()).decode())
"
```

Place `updater_public_key.bin` at `src/services/updater_public_key.bin`. Store the printed base64 private key as the `UPDATE_SIGNING_KEY` GitHub secret.

## Release pipeline

The `.github/workflows/release.yml` workflow handles the full release cycle:

1. **Build**: Compiles a release binary, creates a `.app` bundle, codesigns (or ad-hoc signs), and packages a DMG.
2. **Publish**: Creates a GitHub Release with the DMG attached.
3. **Sign**: `scripts/generate-manifest.sh` computes the asset SHA-256, signs it with `UPDATE_SIGNING_KEY`, and produces `manifest.json`.
4. **Deploy**: Pushes `manifest.json` to the `gh-pages` branch.

Trigger a release by pushing a tag (`git tag v0.3.0 && git push origin v0.3.0`) or using the workflow dispatch.

## Reading state in the UI

The `UpdateSnapshot` global holds the current update state:

```rust
let snap = updater::snapshot(cx);
match &snap.status {
    UpdateStatus::Available { version, notes } => {
        // Show "Update available: v{version}" with notes
    }
    UpdateStatus::Downloading { progress } => {
        // Show progress bar at {progress}%
    }
    UpdateStatus::Downloaded { version, .. } => {
        // Show "Restart to install v{version}"
    }
    UpdateStatus::Error(err) => { /* show error */ }
    _ => {}
}
```

The status bar (`src/shell/status_bar.rs`) renders these states. You can use it as a reference.

## Related docs

- [Notifications](/docs/notifications/) - How update prompts are delivered to the user
- [Getting Started](/docs/getting-started/) - Project setup and structure overview
- [Architecture](/docs/architecture/) - How services, globals, and actions fit together
- [CI/CD for Rust desktop apps](/blog/rust-desktop-ci-cd-guide/) - Full guide to the release workflow
- [Rust desktop app distribution](/blog/rust-desktop-app-distribution/) - Packaging, signing, and notarization
- [Rust desktop security guide](/blog/rust-desktop-security-guide/) - Signing, verification, and secure delivery
