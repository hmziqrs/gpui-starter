# gpui-starter

A full-featured desktop application boilerplate built with [GPUI](https://github.com/zed-industries/zed) from Zed, for macOS.

<!-- Add screenshot here -->

## Prerequisites

- **Rust nightly** (edition 2024)
- **macOS** (primary target; Linux X11/Wayland experimental)

## Quick Start

Build and run directly:

```sh
cargo build
cargo run
```

Or use the macOS development script, which builds the binary, bundles it into an `.app`, and codesigns it:

```sh
bash scripts/macos-dev-app.sh
# Outputs the path to GPUI Starter.app
open "$(bash scripts/macos-dev-app.sh)"
```

### Linux Development

On Debian/Ubuntu, install the build dependencies before building:

```sh
sudo apt update
# Test on Ubuntu 24.04
sudo apt install -y \
  gcc g++ clang libfontconfig-dev libwayland-dev \
  libwebkit2gtk-4.1-dev libxkbcommon-x11-dev libx11-xcb-dev \
  libssl-dev libzstd-dev \
  vulkan-validationlayers libvulkan1
```

## Features

### Core

- **Lifecycle** -- startup / shutdown / crash state machine with first-run detection
- **Single instance** -- IPC guard prevents duplicate processes and forwards deep links
- **Window management** -- custom title bar with drag regions and native traffic-light buttons

### UI

- **Sidebar** -- collapsible navigation with page routing
- **Title bar** -- replaces native window chrome, supports macOS traffic lights
- **Status bar** -- contextual status display at the bottom of the window
- **Command launcher** -- Cmd+K palette with fuzzy search across all app actions
- **Undo / redo** -- command-pattern stack with Cmd+Z / Cmd+Y support

### Data

- **SQLite** -- local data persistence via `rusqlite`
- **Configuration** -- JSON config with migration system for schema changes
- **Secure storage** -- OS keyring integration (macOS Keychain, Windows Credential Manager, Linux Secret Service)

### System

- **Tray** -- macOS system tray with app icon and quick-access menu
- **Notifications** -- native OS backends with in-app toast fallback and persistent inbox
- **Keyboard shortcuts** -- global hotkey (Alt+Space) and app-level keybindings
- **Deep links** -- URL scheme handling with single-instance forwarding

### Internationalization

- **Fluent-based** -- built on `es-fluent` for type-safe translations
- **Locales** -- English (`en`) and Chinese Simplified (`zh-CN`) included

### Developer Experience

- **Logging** -- file-based logging via `tracing-appender`
- **Telemetry** -- disabled / local / remote modes with consent gate
- **Diagnostics** -- live app state, subsystem status, and debug actions page
- **Tests** -- integration test harness under `#[cfg(test)]`
- **Accessibility** -- AccessKit integration for screen reader support

## Architecture

See [docs/gpui-architecture.md](docs/gpui-architecture.md) for an overview of the module layout and data flow.

## Themes

23 built-in themes with live hot-reloading -- drop a JSON file into `themes/` and it appears instantly. Highlights include:

- Gruvbox, Tokyonight, Catppuccin, Everforest
- Solarized, Ayu, Molokai, Jellybeans
- Matrix, Fahrenheit, Hybrid
- macOS Classic, High Contrast (Dark / Light)

Custom themes use the same JSON format. See the `themes/` directory for examples.

## License

[MIT](LICENSE)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.
