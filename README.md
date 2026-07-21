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

On Debian/Ubuntu, install the build dependencies before building. (GPUI does
**not** use WebKit or libzstd — those are leftover from a Tauri template. The
real GPUI stack is Wayland/X11 + Vulkan + FreeType/fontconfig.)

```sh
sudo apt update
# Verified on Ubuntu 24.04
sudo apt install -y \
  gcc g++ clang pkg-config \
  libfontconfig-dev libfreetype-dev \
  libwayland-dev wayland-protocols \
  libxkbcommon-x11-dev libx11-xcb-dev \
  libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libssl-dev \
  libvulkan-dev libvulkan1
```

> `libvulkan-dev` is the **build** header; the actual Vulkan loader
> (`libvulkan1`) is a **runtime** requirement — a release binary built here will
> fail to start on a machine without a Vulkan loader installed. The bundled
> `flake.nix` and `scripts/install-linux-deps.sh` cover both.

#### Nix

A `flake.nix` is provided for reproducible Linux builds. Enter a fully set-up
dev shell (Vulkan/Wayland/XCB on `LD_LIBRARY_PATH`) with:

```sh
nix develop      # then `cargo run`
nix build .#default   # produces a release binary in result/bin/gpui-starter
```

`nix flake lock` must be run once to pin nixpkgs (the lockfile is not shipped).

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
