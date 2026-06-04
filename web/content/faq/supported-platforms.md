---
question: "Does GPUI work on Windows, Linux, and macOS?"
description: "GPUI supports macOS (Metal) and Linux (Vulkan) today. Windows support is in active development. gpui-starter runs on every platform GPUI supports with no extra configuration."
category: "Getting Started"
order: 4
---

## Platform support matrix

| Platform | Status | GPU backend | Notes |
|----------|--------|-------------|-------|
| **macOS** | Stable | Metal | Primary target. Tested on every commit. |
| **Linux** | Stable | Vulkan | X11 and Wayland both supported. Requires Vulkan-capable GPU and dev headers. |
| **Windows** | In development | DirectX | Compiles but has rendering gaps. Not ready for production use. |

## macOS

macOS is where GPUI was born and where it gets the most testing. The Zed editor (the framework's reference app) ships on macOS as its primary platform. Metal rendering is mature and stable. All gpui-starter features work here: custom title bar, system tray, global hotkeys, keychain integration, native notifications, and auto-update with Ed25519 signed manifests.

Minimum version: macOS 12 Monterey. The Metal feature set GPUI relies on requires it.

## Linux

Linux support is stable. GPUI renders through Vulkan and handles both X11 and Wayland display servers. You need the system development headers installed. On Ubuntu and Debian:

```bash
sudo apt install libx11-dev libwayland-dev libvulkan-dev
```

On Fedora:

```bash
sudo dnf install libX11-devel wayland-devel vulkan-loader-devel
```

If you are running a headless server or a VM without GPU passthrough, you will not get a working window. GPUI needs a real GPU or a software Vulkan renderer like Mesa's lavapipe.

The [getting started guide](/docs/getting-started/) has the full list of prerequisites for each platform.

## Windows

Windows support is under active development by the Zed team. The DirectX backend compiles and renders basic windows, but you will hit missing features, visual glitches, and crashes in more complex layouts. If you want to contribute or track progress, the Zed repository has open issues labeled for Windows platform work.

gpui-starter compiles on Windows, but I would not recommend it for production until the upstream GPUI support solidifies.

## What this means for gpui-starter

gpui-starter itself has no platform-specific configuration. The [architecture](/docs/architecture/) isolates OS interaction behind the `platform/` module (tray, credentials, process locking, networking), so the rest of the codebase does not care which OS it runs on. When GPUI adds full Windows support, the only changes needed will be inside that platform layer.

## How GPUI compares to other Rust GUI frameworks on platform coverage

If cross-platform day-one support is your top priority, Tauri and egui cover more platforms today. The [Rust GUI frameworks comparison](/blog/rust-gui-frameworks-comparison-2026/) breaks down where each framework stands. GPUI trades broader platform support for rendering performance and a capable retained-mode API. For a deeper look at that tradeoff, see [GPUI vs egui vs iced](/blog/gpui-vs-egui-vs-iced/) and [Electron vs Tauri vs GPUI](/blog/electron-vs-tauri-vs-gpui/).

When you are ready to distribute your app on the platforms GPUI supports, the [distribution guide](/blog/rust-desktop-app-distribution/) covers codesigning, notarization, and packaging for each OS.
