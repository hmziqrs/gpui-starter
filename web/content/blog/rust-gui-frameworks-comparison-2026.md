---
title: "Rust GUI frameworks in 2026: the complete comparison guide"
description: "Every Rust GUI framework compared: GPUI, Tauri, egui, iced, Slint, Dioxus, Xilem, Vizia. Features, maturity, and best use cases."
date: 2026-06-05
tags: [Rust, GUI, comparison]
draft: false
---

I've spent the last year building desktop applications in Rust, and one question comes up more than any other: which GUI framework should I use? The Rust GUI ecosystem has grown fast, and there are now eight frameworks worth serious consideration. Each makes different tradeoffs, and picking the wrong one can cost you months. This is where each one stands in 2026.

## The eight contenders

The frameworks covered here are GPUI, Tauri, egui, iced, Slint, Dioxus, Xilem, and Vizia. I'm excluding bindings to C++ toolkits like Qt or GTK because they don't give you a fully Rust-native development experience. If you want that, cxx-qt exists, but it's a different category.

## egui: immediate mode, zero friction

egui is an immediate mode framework. Every frame, you describe what the UI should look like, and egui figures out the rest. No state machines or message passing. No architectural ceremony.

```rust
ui.horizontal(|ui| {
    ui.label("Filter:");
    let response = ui.text_edit_singleline(&mut filter);
    if ui.button("Apply").clicked() {
        apply_filter(&filter);
    }
});
```

What egui gets right is speed of development. You can build a functional tool in hours. The API is well-documented and discoverable. If you have ever used Dear ImGui in C++, egui will feel familiar immediately.

Where egui falls short is layout and styling. egui's layout engine is basic. Complex responsive layouts are hard. Custom styling is possible but fights the grain. The framework is optimized for developer tools, not consumer applications.

**Best for:** Game debug overlays, internal tools, scientific instrument interfaces, rapid prototypes.

## iced: Elm architecture in Rust

iced implements the Elm architecture: state, update, view. Messages flow from user interactions through an update function, producing new state, which produces a new view. If you have used Elm or Redux, this pattern is second nature.

```rust
fn update(&mut self, message: Message) -> Command<Message> {
    match message {
        Message::EmailChanged(email) => self.email = email,
        Message::Submit => {
            if validate(&self.email) {
                return Command::perform(submit_form(self.email.clone()), Message::Submitted);
            }
        }
        Message::Submitted(result) => self.status = result,
    }
    Command::none()
}

fn view(&self) -> Element<Message> {
    column![
        text_input("Email", &self.email).on_input(Message::EmailChanged),
        button("Submit").on_press(Message::Submit),
    ].into()
}
```

iced's architecture is clean and testable. It also has a web backend via WebAssembly, which is unique among the frameworks on this list. You can share core logic between native and web builds.

The problem is pace. iced development has been slow relative to the others. Major features take a long time to ship. The wgpu renderer works but does not feel as fast as GPUI or egui for complex interfaces. Custom widgets require understanding iced's internal rendering pipeline, which is not well-documented.

**Best for:** Apps where architectural correctness matters more than shipping speed. Projects that need web + native code sharing.

## GPUI: GPU-native rendering

GPUI powers Zed, one of the fastest code editors built. It is a retained mode, GPU-accelerated framework written entirely in Rust. The rendering pipeline talks directly to Metal on macOS and Vulkan on other platforms. There is no webview or CSS engine in between.

```rust
fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .p_4()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(gpui::rgb(0x888888))
                .child("Status")
        )
        .child(
            Button::new("action", "Run Task")
                .on_click(cx.listener(|this, _event, cx| {
                    this.run_task(cx);
                }))
        )
}
```

The performance is genuine. Text rendering, layout, scrolling, and compositing all happen on the GPU. Zed opens large files instantly, and a lot of that comes down to GPUI's rendering architecture. The reactive system handles async operations and state updates without requiring tokio for most UI work.

GPUI has real limitations though. It is primarily macOS-focused. Linux support exists but is less polished. Windows support is in progress but not production-ready. The ecosystem is smaller than Tauri's. You will write more code for things that Tauri handles with a single config entry, like tray icons or auto-updates. I wrote about this in more detail in [my GPUI vs egui vs iced comparison](/blog/gpui-vs-egui-vs-iced/).

**Best for:** Performance-critical desktop apps, editors, creative tools, anything where frame budget matters.

## Tauri: web frontend, Rust backend

Tauri 2.0 is the most mature option for developers who want to write their frontend in familiar web technologies. It uses the system webview (WebKit on macOS, WebView2 on Windows, WebKitGTK on Linux) instead of bundling Chromium. This gives you small binaries, typically under 10MB, and reasonable memory usage around 30 to 50MB at idle.

The Rust side handles all the backend logic through a command system:

```rust
#[tauri::command]
async fn fetch_data(query: String) -> Result<Vec<Item>, String> {
    let results = database::search(&query)
        .await
        .map_err(|e| e.to_string())?;
    Ok(results)
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![fetch_data])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Tauri 2.0 also supports mobile targets. You can build for iOS and Android from the same codebase. The plugin system covers notifications, filesystem access, auto-updates, deep linking, and more. There is a large community and a lot of third-party packages.

The tradeoff is that you are still dealing with webview inconsistencies across platforms. Rendering behavior, JavaScript performance, and bugs all vary between webviews. Complex layouts and animations will hit the same DOM performance ceiling that any web app hits. I covered this in more depth in [my Tauri vs Electron performance breakdown](/blog/tauri-vs-electron-performance/).

**Best for:** Cross-platform apps where you want to use web skills. Internal tools. Apps that need mobile support.

## Slint: the commercial option

Slint is a declarative UI framework with its own domain-specific markup language. You define your interface in `.slint` files and write business logic in Rust, C++, or JavaScript.

```rust
// In markup
// export component AppWindow inherits Window {
//     in-out property <string> name;
//     callback greet;
//     VerticalLayout {
//         TextInput { text <=> name; }
//         Button { text: "Greet"; clicked => { greet(); } }
//     }
// }

// In Rust
let app_window = AppWindow::new()?;
app_window.on_greet(|| {
    println!("Hello!");
});
```

Slint is polished. The tooling includes a live preview designer and good documentation, with consistent cross-platform rendering. It runs on desktop, embedded systems, and microcontrollers. That embedded support is unique among these frameworks.

The catch is licensing. Slint is dual-licensed: GPLv3 for open source, or a commercial license if you need to keep your code proprietary. For some projects this is fine. For others, it is a dealbreaker. The community is smaller than Tauri's or egui's, partly because of this.

**Best for:** Embedded UIs, industrial applications, teams that want commercial support and do not mind the license.

## Dioxus: React for Rust

Dioxus brings React-style component patterns to Rust. If you know React, you can be productive in Dioxus quickly.

```rust
fn App() -> Element {
    let mut count = use_signal(|| 0);

    rsx! {
        div {
            h1 { "Counter: {count}" }
            button { onclick: move |_| count += 1, "Increment" }
            button { onclick: move |_| count -= 1, "Decrement" }
        }
    }
}
```

Dioxus supports multiple renderers: web via WebAssembly, desktop via webview (similar to Tauri), and a terminal renderer. The server-side rendering support makes it possible to build full-stack Rust applications. The component model, with hooks and props, is well-designed.

The desktop renderer still relies on webviews, so it inherits the same performance characteristics as Tauri. The framework is evolving fast, which means occasional breaking changes. Documentation has improved but still has gaps compared to Tauri or egui.

**Best for:** Developers with React experience who want to stay in Rust. Full-stack Rust projects.

## Xilem: the research project

Xilem is the experimental framework from Raph Levien, the person behind the Druid toolkit and much of the text rendering work in the Rust GUI ecosystem. It uses a reactive architecture with a focus on correct, efficient rendering.

Xilem is still early. The architecture is interesting, borrowing ideas from SwiftUI with a tree-diffing approach that minimizes re-rendering. The rendering backend uses Vello, a GPU vector graphics renderer that produces excellent results.

Right now, Xilem is not production-ready. The API is unstable, documentation is sparse, and the ecosystem does not exist yet. But the technical foundations are strong, and it is worth watching if you care about where Rust GUI is headed.

**Best for:** Experimentation. Contributing to the future of Rust GUI. Not for shipping products yet.

## Vizia: the mid-tier option

Vizia is a retained mode framework that sits somewhere between iced and GPUI in terms of API style. It uses a declarative approach with a focus on data binding.

```rust
fn build(cx: &mut Context) {
    Label::new(cx, "Hello")
        .background_color(Color::rgb(240, 240, 240));
}
```

Vizia supports multiple rendering backends including femtovg and wgpu. It also handles localization through the Fluent system. The framework is usable but has a smaller community than the others on this list.

The main concern with Vizia is long-term maintenance. With fewer contributors and less corporate backing than Tauri, egui, or GPUI, there is more risk that development slows or stalls.

**Best for:** Mid-complexity desktop apps where you want something between egui's simplicity and iced's architecture.

## How to decide

My decision framework, based on what I have actually built with these tools:

- **Need to ship fast with web skills?** Tauri 2.0. The ecosystem is large, mobile support exists, and you already know HTML/CSS/JS.
- **Building a performance-critical desktop app?** GPUI. The GPU-native rendering makes a real difference. See [building desktop apps with Rust and GPUI](/blog/building-desktop-apps-rust-gpui/) for the full case.
- **Making a tool for yourself or your team?** egui. You will have something working today, not next week.
- **Need embedded support?** Slint. Nothing else here runs on microcontrollers.
- **Care about architectural purity?** iced. The Elm model is clean and testable.
- **Coming from React?** Dioxus. The mental model transfers directly.

There is no single right answer. I picked GPUI for my own project because I wanted GPU-native rendering and was willing to accept the macOS-first limitation. For a cross-platform business app with a small team, I would probably choose Tauri. For a weekend tool, egui.

## The practical starting point

If you want to see what a real GPUI application looks like, with multi-page navigation, theming, i18n, form validation, auto-updates, and the rest of the boilerplate you need for a production app, check out [gpui-starter on GitHub](https://github.com/hmziqrs/gpui-boilerplate) and the [getting started guide](/docs/getting-started/). It handles the setup work so you can focus on building your actual application.
