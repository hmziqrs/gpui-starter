---
title: "GPUI vs egui vs iced: Rust GUI frameworks compared"
description: "A practical comparison of egui, iced, and GPUI for Rust desktop apps. Covers rendering, performance, learning curve, and when each framework is the right pick."
date: 2026-05-29
tags: [Rust, GPUI, comparison]
draft: false
---

If you want to build a desktop app in Rust, you have a real choice to make. egui, iced, and GPUI are the three frameworks people actually ship with, and they take different approaches. I've used all three. Here's what each one does well, where each one falls short, and why I ended up picking GPUI for [gpui-starter](/docs/getting-started/) despite its rough edges.

## egui: the pragmatic choice

egui is an immediate mode GUI framework. Every frame, you describe what the interface should look like right now, and egui handles the rest. No state management scaffolding, no message passing, no architecture to learn. You just write UI code.

```rust
ui.horizontal(|ui| {
    ui.label("Your name:");
    let response = ui.text_edit_singleline(&mut name);
    if ui.button("Click").clicked() {
        // handle the click
    }
});
```

That simplicity is egui's biggest strength. If you've written ImGui code in C++, you'll feel at home. The API is discoverable. The docs are solid. You can prototype something usable in an afternoon.

The downside is that immediate mode comes with real limitations. Layout is basic. Complex layouts, custom styling, and animations all fight the grain of the library. egui renders through a backend (usually wgpu), and it works fine, but it's not trying to compete on rendering performance.

egui is the right call for tools, debug overlays, game editors, and internal utilities. If you need a GUI on some Rust logic quickly and don't care how it looks, egui gets you there fast.

## iced: the principled option

iced uses the Elm architecture. Your app is divided into state, update logic, and view. Messages flow from the view through the update function, producing new state, which produces a new view. If you've used Elm or Redux, you know this pattern.

```rust
fn update(&mut self, message: Message) -> Command<Message> {
    match message {
        Message::Increment => self.value += 1,
        Message::Decrement => self.value -= 1,
    }
    Command::none()
}

fn view(&self) -> Element<Message> {
    column![
        button("Increment").on_press(Message::Increment),
        text(self.value).size(50),
        button("Decrement").on_press(Message::Decrement),
    ].into()
}
```

The architecture is clean and testable. State mutations are explicit. iced also has a web backend, useful if you want to share code between native and web. The documentation is probably the best of the three frameworks.

The problem with iced is development velocity. The project moves slowly. Major features take a long time to land. The native renderer isn't fast enough for complex interfaces, and even the wgpu backend doesn't feel as snappy as a proper GPU-first renderer. Custom widgets require understanding the internals, and the learning curve is steeper than egui even though the architecture looks cleaner on paper.

iced is a good fit if you value architectural correctness and don't mind waiting for features. It's the best option if you need a web-compatible GUI layer in Rust.

### GPUI: the production engine

GPUI is the framework that powers Zed, the code editor. It's a retained mode, GPU-accelerated UI framework written entirely in Rust. GPUI renders directly through the GPU using a custom rendering pipeline. This isn't a CPU renderer with a GPU backend bolted on. The rendering is GPU-native from the start.

```rust
fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
    div()
        .flex()
        .p_4()
        .gap_2()
        .child("Hello, GPUI")
        .child(
            Button::new("click-me", "Click me")
                .on_click(|_event, cx| {
                    println!("clicked");
                })
        )
}
```

The performance is real. Zed is fast partly because GPUI is fast. Text rendering, layout, scrolling, and compositing all happen on the GPU. The framework handles async through its own reactive system. You don't need to bolt on tokio or worry about threading for most UI work.

But GPUI is not easy to learn. The documentation is sparse. The API surface is large and sometimes inconsistent. There are concepts like `Model`, `View`, `Entity`, and `Context` that take time to understand. I wrote about the [entity system in detail](/blog/gpui-entity-system-explained/) because the existing docs didn't cover it well enough. The framework is opinionated about how you structure your app, and fighting those opinions is painful.

GPUI is also less mature as a standalone framework. It was built for Zed first and extracted second. Some APIs feel like they were designed for an editor rather than general-purpose GUI work. The ecosystem is smaller, with fewer examples and third-party components.

For a production desktop application where rendering performance and responsiveness matter, GPUI is the best option in Rust right now. You can see what it's capable of by looking at the [rendering pipeline breakdown](/blog/gpui-rendering-pipeline-deep-dive/) or the [full framework comparison for 2026](/blog/rust-gui-frameworks-comparison-2026/).

## how they compare

| | egui | iced | GPUI |
|---|---|---|---|
| rendering | immediate mode | retained (Elm) | retained (reactive) |
| GPU acceleration | optional (wgpu backend) | optional (wgpu backend) | always on |
| maturity | high | medium | medium |
| ecosystem | large | medium | small |
| learning curve | low | medium | high |
| best for | tools, debug UIs | web+native apps | production desktop apps |
| styling | limited | decent | flexible (CSS-like) |
| async support | manual | built-in | built-in |

## when to pick what

Pick egui when you need something fast and don't care about polish. Game tools, debug visualizations, internal dashboards, one-off utilities. The API is simple enough that you can ship a usable tool in a day.

Pick iced when architecture matters more than raw performance. If testability, state management, and code organization are primary concerns, the Elm architecture pays off. iced is also the only option here with a web target.

Pick GPUI when you're building a real desktop application that needs to feel fast and native. Text editors, IDEs, creative tools, anything with complex layout and high frame rate requirements. You'll pay for it in learning time, but the result is a faster app.

## why GPUI for gpui-starter

I picked GPUI because gpui-starter is meant for people building desktop applications. Not tools, not web apps, not prototypes. Real applications where users expect responsive interfaces and native-feeling interactions.

The other frameworks could work for some of those cases. But GPUI gives you the right foundation for a desktop app that needs complex layout, smooth scrolling, text rendering, and real-time updates without dropping frames. The fact that Zed exists as proof of concept matters. You can look at Zed and see what GPUI is capable of.

The tradeoff is real. GPUI's documentation needs work. The learning curve is steep. The framework is still evolving. If you want a gentle introduction to Rust GUI development, start with egui instead. But if you want to build something that feels like a proper desktop application and you're willing to invest the time, GPUI is worth it.

If you want to try it yourself, the [getting started guide](/docs/getting-started/) walks through setting up a GPUI project from scratch. For a deeper look at how the framework is structured, the [architecture page](/docs/architecture/) covers the core concepts. And if you're weighing GPUI against other options, I also wrote a separate comparison of [GPUI vs Tauri](/blog/gpui-vs-tauri-framework/) and [Electron alternatives for 2026](/blog/electron-alternatives-2026/) that might help you decide.
