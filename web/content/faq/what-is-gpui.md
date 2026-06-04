---
question: "What is GPUI and how does it work?"
description: "GPUI is Zed's GPU-accelerated UI framework for Rust. It renders directly via Metal and Vulkan, uses a retained entity system for state, and ships production code inside the Zed editor."
category: "Getting Started"
order: 1
---

GPUI is a **GPU-accelerated UI framework** for Rust, written by the [Zed](https://zed.dev) team to power their code editor. Unlike Electron apps that run a Chromium instance, or native toolkits that delegate to platform widgets, GPUI renders your entire interface through GPU pipelines: Metal on macOS, Vulkan via wgpu on Linux.

## How rendering works

Every frame goes through three phases before pixels hit the screen:

1. **Layout** - Taffy (a Flexbox engine) computes sizes and positions for every element
2. **Prepaint** - elements commit their bounds for hit-testing and text shaping
3. **Paint** - visual primitives (quads, paths, glyphs, sprites) get pushed into a `Scene` buffer, then submitted to the GPU in batched draw calls

Your `render()` method is a pure projection of state to elements. Mutate state through the entity system, call `cx.notify()`, and GPUI re-renders only what changed. See the [architecture guide](/docs/architecture/) for how entities and globals fit together, or read the [rendering pipeline deep dive](/blog/gpui-rendering-pipeline-deep-dive/) for what happens inside the GPU.

## The entity system

State in GPUI lives inside `Entity<T>` handles. An entity wraps your data, tracks observers, and schedules re-renders when you call `cx.notify()`. There is no virtual DOM, no diffing algorithm. When state changes, the owning view re-renders. The [entity system explained](/blog/gpui-entity-system-explained/) covers `Model`, `View`, `Entity`, and `Context` in detail with real code.

## What GPUI gives you

- **Retained mode API** - you describe *what* the UI looks like, GPUI handles the drawing
- **Declarative elements** - composition via `div()`, `flex()`, and custom `Element` trait implementations
- **Async-first runtime** - built-in futures support through the entity system, no external runtime needed
- **Pure Rust** - no JavaScript, no C++ bindings, no FFI bridge
- **Accessibility** - integrates with [AccessKit](https://accesskit.dev/) for screen reader support

## GPUI compared to other Rust UI options

egui uses immediate mode (you re-emit the full UI every frame). iced follows an Elm architecture with messages and updates. GPUI uses retained entities with direct GPU rendering. Each approach has tradeoffs. The [GPUI vs egui vs iced comparison](/blog/gpui-vs-egui-vs-iced/) breaks down rendering performance, learning curve, and when each framework makes sense. If you are coming from Electron or Tauri, the [framework comparison](/blog/electron-vs-tauri-vs-gpui/) covers architecture and resource usage differences.

## Production readiness

GPUI runs inside Zed, a code editor used by thousands of developers daily. It handles syntax highlighting, multi-file editing, terminal emulation, and collaborative editing under real workloads. The framework itself lives in the [zed repository](https://github.com/zed-industries/zed) under the `crates/gpui` directory.

gpui-starter builds on top of GPUI with a pre-configured project structure: multi-page navigation, 21 themes, i18n, form validation, command launcher, and more. See [getting started](/docs/getting-started/) to run it, or read [why gpui-starter](/faq/why-gpui-starter/) for what the boilerplate includes.
