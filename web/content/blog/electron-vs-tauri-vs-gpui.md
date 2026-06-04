---
title: "Electron vs Tauri vs GPUI: which to pick for your desktop app"
description: "Electron, Tauri, and GPUI compared head to head for desktop app development. Architecture, performance, resource usage, and when to pick each one."
date: 2026-06-05
tags: [Electron, Tauri, GPUI]
draft: false
---

Picking a desktop framework is a decision you live with for years. You can rewrite your backend, swap your database, even change your deployment model. But the UI framework touches every line of your frontend code, every build pipeline, every hiring decision. Getting it wrong is expensive.

I've shipped production apps with all three of these frameworks. This is what I learned from real projects rather than benchmarks.

## The three architectures in plain terms

**Electron** bundles Chromium and Node.js into every app. Your UI is a web page running inside that bundled browser. Your backend logic runs in a Node.js process. They talk over IPC. You already know the stack: HTML, CSS, JavaScript, and whatever frontend framework you prefer.

**Tauri** uses the operating system's built-in webview instead of shipping Chromium. Your UI is still a web page, but it renders through WebView2 on Windows, WebKit on macOS, or WebKitGTK on Linux. The backend is Rust, not Node.js. IPC connects them through async message passing with a permission-based security model.

**GPUI** is something different entirely. There is no browser, no webview, no HTML, no CSS, no JavaScript. You write Rust that describes UI elements, and GPUI renders directly to the GPU through Metal on macOS and Vulkan on other platforms. It was built by the [Zed](https://zed.dev) team for their editor.

## Electron: the safe default

Electron's biggest advantage is that every web developer already knows how to use it. Your team can start building on day one with React, Vue, Svelte, or plain DOM manipulation. The ecosystem is enormous. Need OAuth? There is a library. Need to parse Excel files? There is a library. Need to talk to a serial port? There is a library.

VS Code proved that you can build a good application in Electron. Slack, Discord, Notion, and Figma all ship with it. The npm ecosystem means you are never more than `npm install` away from a solution.

The cost shows up in resources. A minimal Electron hello world is 130MB on disk. Idle RAM starts at 80 to 120MB. Slack with a few workspaces hits 1GB. Discord sits around 500MB. These are not bugs. They are the consequence of bundling an entire browser into every app.

```javascript
// Electron: your main process
const { app, BrowserWindow } = require('electron');

app.whenReady().then(() => {
  const win = new BrowserWindow({ width: 800, height: 600 });
  win.loadFile('index.html');
});
```

That snippet works. It ships. The DX is hard to beat.

Where Electron hurts: startup time, memory pressure on machines with 8GB RAM, and the constant feeling that your app is a web page pretending to be native. Scroll physics feel wrong. Text rendering does not match the OS. Window chrome looks off unless you spend real effort on custom titlebars.

**Pick Electron when:** your team only knows web technologies, you need to ship fast, and resource usage is not a dealbreaker. For internal tools and B2B apps where the user is already running Chrome anyway, Electron is fine.

## Tauri: the leaner web approach

Tauri's core insight is that every major OS already ships a webview. Why bundle Chromium when the user has one installed? That single decision drops binary size from 130MB to around 3 to 8MB. Idle RAM drops to 30 or 40MB.

The backend is Rust instead of Node.js. If your team is comfortable with Rust, this is a genuine upgrade. You get memory safety, predictable performance, and a real type system. The IPC layer uses async message passing with a permission model that is stricter than Electron's. By default, the frontend can access nothing. You explicitly grant access to specific capabilities.

```rust
// Tauri: a Rust command exposed to the frontend
#[tauri::command]
fn read_config(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
}
```

```javascript
// Frontend calls it through IPC
const config = await invoke('read_config', { path: '/etc/app.conf' });
```

The tradeoff is the webview itself. WebView2, WebKit, and WebKitGTK are three different rendering engines with three different sets of bugs and quirks. Cross-platform testing becomes testing against three browsers. You will hit rendering differences. Text will look slightly different on each platform. Animation timing will vary. It is better than Electron's resource usage, but you still carry the fundamental overhead of a DOM, a CSS engine, and a JavaScript runtime.

Tauri 2 also added mobile support (iOS and Android), which Electron does not have. If sharing code between desktop and mobile matters to you, that is a real point in Tauri's favor.

**Pick Tauri when:** you want web-tech DX with better resource usage and security, your backend logic benefits from Rust, or you need mobile support from the same codebase.

## GPUI: native GPU rendering

GPUI takes the most radical approach. No web technologies at all. You write Rust, describe UI as a tree of elements, and GPUI renders directly to the GPU. No DOM to diff, no CSS to parse, no JavaScript engine to warm up.

The rendering pipeline has three phases: layout (flexbox-based sizing and positioning), prepaint (hitboxes, text shaping, spatial data), and paint (batched GPU draw calls for quads, text, and decorations). Because there is no browser layer, the path from "your code changed" to "pixels updated" is short and predictable.

```rust
struct Counter {
    count: usize,
}

impl Render for Counter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .gap_2()
            .child(
                div()
                    .text_lg()
                    .child(format!("Count: {}", self.count))
            )
            .child(
                div()
                    .px_4()
                    .py_2()
                    .bg(gpui::blue())
                    .text_color(gpui::white())
                    .cursor_pointer()
                    .child("Increment")
                    .on_click(cx.listener(|this, _, _cx| {
                        this.count += 1;
                    }))
            )
    }
}
```

That is a working interactive component. No JSX, no virtual DOM, no state management library. The `on_click` closure captures a listener that mutates state and triggers a re-render.

The Zed editor opens instantly, scrolls through multi-gigabyte files, and handles syntax highlighting at 120fps. It does this because there is nothing between the rendering code and the GPU. No DOM diffing, no style recalculation cascade, no JavaScript garbage collection pauses.

The downside is maturity. GPUI is younger than Electron and Tauri by years. The ecosystem is smaller. There are fewer tutorials, fewer third-party components, and fewer people on Stack Overflow who can answer your questions. The API is still evolving. You will read source code. You will file issues. You will occasionally hit the edges of what the framework supports.

It also only supports macOS and Linux right now. Windows support is in progress but not production-ready. If you need to ship on Windows tomorrow, GPUI is not your choice today.

If you want a deeper comparison with other Rust GUI frameworks specifically, I wrote about that in [GPUI vs egui vs iced](/blog/gpui-vs-egui-vs-iced/).

**Pick GPUI when:** you need maximum rendering performance, you are comfortable with Rust, macOS and Linux cover your target users, and you are willing to deal with a younger ecosystem.

## The decision matrix

| Factor | Electron | Tauri | GPUI |
|---|---|---|---|
| Binary size | 130MB+ | 3 to 8MB | 5 to 15MB |
| Idle RAM | 80 to 120MB | 30 to 40MB | 10 to 20MB |
| Startup time | 2 to 5s | 0.5 to 1.5s | under 0.5s |
| Language | JS/TS + any | JS/TS + Rust | Rust only |
| Platforms | Win, Mac, Linux | Win, Mac, Linux, mobile | Mac, Linux (Win in progress) |
| Learning curve | Low if you know web | Medium | High |
| Ecosystem | Massive | Growing | Small but active |

These numbers come from building comparable apps in each framework and profiling them on the same hardware. Your mileage will vary with app complexity, but the relative ordering holds.

## What I actually recommend

For a startup shipping an MVP to non-technical users, Electron is hard to argue against. Your team can move fast. The resource cost annoys power users, but most people do not notice or care.

For a tool where resource usage and security matter, Tauri is the sweet spot. You keep the web frontend your designers know, but you get Rust on the backend and a much lighter footprint.

For a performance-sensitive application, like an editor, a creative tool, or anything doing real-time rendering, GPUI is worth the investment. The upfront cost of learning the framework pays off every time your app renders a frame without jank.

I ended up building [gpui-starter](/docs/getting-started/) on GPUI because I wanted the rendering performance and I was willing to accept the smaller ecosystem. The project includes multi-page navigation, 21 themes, i18n, form validation, a command palette, and a bunch of other things that you would otherwise have to build from scratch. If you are considering GPUI for a real project, it is a reasonable starting point. The full source is on [GitHub](https://github.com/hmziqrs/gpui-boilerplate).
