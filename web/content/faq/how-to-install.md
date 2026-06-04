---
question: "How do I install and set up a GPUI Rust desktop project?"
description: "Step-by-step guide to cloning, building, and running gpui-starter. Covers Rust toolchain requirements, system dependencies for macOS and Linux, common build errors, and project structure."
category: "Getting Started"
order: 3
---

## What you need first

**Rust 1.85 or later** with the 2024 edition. If you don't have Rust installed, use [rustup](https://rustup.rs):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

GPUI renders through the native GPU compositor on each platform, so you need the system libraries that sit between your code and the hardware:

- **macOS**: Xcode Command Line Tools. Run `xcode-select --install` and accept the prompt. That is all you need on Apple Silicon and Intel Macs alike.
- **Linux (Ubuntu/Debian)**: Vulkan headers, X11 client libraries, and a few XCB extensions:

```bash
sudo apt install libvulkan-dev libx11-dev libxcb1-dev libxcb-keysyms1-dev libxkbcommon-dev
```

Other distros need the same libraries under different package names. On Fedora: `vulkan-devel libX11-devel xcb-util-keysyms-devel libxkbcommon-devel`. On Arch: `vulkan-devel libx11 xcb-util-keysyms libxkbcommon`.

## Clone and build

```bash
git clone https://github.com/hmziqrs/gpui-boilerplate.git gpui-app
cd gpui-app
cargo run
```

The first `cargo run` compiles GPUI and roughly 200 crate dependencies. On a modern machine this takes 3-5 minutes. After that, incremental builds finish in seconds.

If you want to build in release mode for actual performance testing:

```bash
cargo run --release
```

Release builds take longer to compile but run noticeably faster because GPUI's layout and draw loops get full LLVM optimization.

## What the project structure looks like

After cloning, the directories that matter day-to-day:

| Path | What it contains |
|------|-----------------|
| `src/` | Application code: main entry point, views, services |
| `themes/` | JSON theme files, hot-reloaded at runtime |
| `locales/` | Translation files for i18n (English and Chinese by default) |
| `migrations/` | SQLite schema migrations |
| `Cargo.toml` | Dependencies and feature flags |

The [Architecture](/docs/architecture/) doc goes deeper into how modules connect. For a tour of the default feature set, see [Getting Started](/docs/getting-started/).

## Common build problems

**`linker 'cc' not found` on Linux**: Install `build-essential` (`sudo apt install build-essential`). The C linker is required by some GPUI dependencies.

**Vulkan compilation errors on Linux**: Double-check that `libvulkan-dev` is installed and your GPU driver supports Vulkan. Run `vulkaninfo | head` to verify. If that command fails, your driver or loader is missing.

**macOS: "no developer tools found"**: Run `xcode-select --install`. If you have Xcode installed but still see this, run `sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer`.

**Slow first build**: Normal. GPUI pulls in `metal` on macOS and `vulkan`/`x11` bindings on Linux, all of which are large crates. The [Rust GUI best practices guide](/blog/rust-gui-best-practices-2026/) has tips for keeping compile times manageable as your project grows.

## Next steps

Once the app is running:

- Read the [Getting Started](/docs/getting-started/) guide for a walkthrough of every built-in feature.
- Browse the [theme system](/docs/themes/) to customize colors and typography.
- Check out [Hello World in GPUI](/blog/hello-world-gpui-tutorial/) if you want to understand how GPUI rendering works from scratch.
- For planning a real product, the [prototype to production](/blog/scaling-gpui-prototype-to-production/) post covers the architectural decisions that matter once you move past a demo.
- Ready to ship? The [Rust desktop distribution guide](/blog/rust-desktop-app-distribution/) walks through codesigning, notarization, and auto-updates.
