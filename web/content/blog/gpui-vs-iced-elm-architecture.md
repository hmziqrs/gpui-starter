---
title: "GPUI vs iced: reactive vs Elm architecture in Rust GUI"
description: "Comparing GPUIs reactive entity model with iceds Elm architecture for building Rust desktop GUIs, with code examples and tradeoffs."
date: 2026-06-05
tags: [GPUI, iced, comparison]
draft: false
---

I recently rewrote a medium-sized desktop app from iced to GPUI. The hardest part was not learning new APIs or fighting the build system. It was unlearning a mental model I had gotten comfortable with. iced uses the Elm architecture. GPUI uses something closer to reactive entities with observer notifications. Both work. They feel nothing alike.

This post compares the two approaches at the architectural level. I am not interested in benchmarks or feature checklists. I want to explain how each framework wants you to think about state, updates, and rendering, and where those mental models break down.

## The Elm architecture in iced

If you have used Elm, Redux, or MVI (Model-View-Intent) on the web, iced will feel familiar. The architecture is three things:

1. **State** (your data struct)
2. **Update function** (how messages mutate state)
3. **View function** (how state becomes UI)

Messages are the only way state changes. The view emits messages through callbacks. The update function handles each message and returns new state. The framework re-renders from that new state.

Here is a counter in iced:

```rust
use iced::widget::{button, column, text};

#[derive(Debug, Clone)]
enum Message {
    Increment,
    Decrement,
}

fn update(count: &mut u64, message: Message) {
    match message {
        Message::Increment => *count += 1,
        Message::Decrement => *count = count.saturating_sub(1),
    }
}

fn view(count: &u64) -> iced::Element<Message> {
    column![
        button("+").on_press(Message::Increment),
        text(count.to_string()).size(40),
        button("-").on_press(Message::Decrement),
    ]
    .into()
}

fn main() -> iced::Result {
    iced::application("Counter", update, view)
        .run()
}
```

Notice the separation. `update` only touches state. `view` only reads state and returns a description of the UI. They never share mutable context. There is no way for the view to directly modify state because the view function takes `&self`, not `&mut self`.

This is the core selling point. Every state mutation goes through one function. You can trace any change by reading the update function. Testing is straightforward: pass in a state and a message, assert on the output state. No UI harness needed.

## The reactive model in GPUI

GPUI does not have a single update function. It has entities, contexts, and observer notifications. State is stored in `Entity<T>` handles. You mutate state through `entity.update(cx, ...)`. After mutation, you call `cx.notify()` to tell GPUI that something changed and views depending on this entity should re-render.

Here is the same counter in GPUI:

```rust
use gpui::*;

struct Counter {
    count: u64,
}

impl Render for Counter {
    fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        div()
            .flex()
            .gap_2()
            .child(div().child(format!("{}", self.count)))
            .child(
                div()
                    .cursor_pointer()
                    .child("+")
                    .on_click(cx.listener(|this, _event, cx| {
                        this.count += 1;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .cursor_pointer()
                    .child("-")
                    .on_click(cx.listener(|this, _event, cx| {
                        this.count = this.count.saturating_sub(1);
                        cx.notify();
                    })),
            )
    }
}

fn main() {
    App::new().run(|cx| {
        cx.open_window(WindowOptions::default(), |cx| {
            cx.new_view(|_cx| Counter { count: 0 })
        });
    });
}
```

The differences are structural. In iced, callbacks emit `Message` values and the framework routes them to `update`. In GPUI, callbacks are closures that directly mutate the entity and call `cx.notify()`. There is no message enum. There is no central dispatch.

For more on how entities and contexts work, see my post on [state management with GPUI entities](/blog/state-management-gpui-entities/).

## Where the Elm architecture wins

The Elm architecture has real advantages that are not just theoretical.

Testability is better. Your update function is a pure function from `(State, Message) -> State`. You can unit test every state transition without rendering anything. In GPUI, mutations happen inside closures that require a `ViewContext`, which means you need to set up a test context even for simple logic.

```rust
// iced: testing state transitions is this simple
#[test]
fn increment() {
    let mut count = 0u64;
    update(&mut count, Message::Increment);
    assert_eq!(count, 1);
}
```

Debugging is easier to trace. Every state change is a message. You can log every message in one place and reconstruct exactly how your app reached its current state. GPUI has no equivalent because mutations can happen anywhere an `Entity` handle and a `Context` are available.

Serialization and replay are natural. Since messages are enum values, you can serialize them, send them over a network, persist them to disk, and replay them. This is how Elm's time-travel debugging works. iced does not ship a time-travel debugger, but the architecture makes it possible.

Cross-platform rendering is another win. iced has multiple rendering backends including wgpu and a web target. The Elm architecture decouples your app logic from the rendering layer. You can write your update and view logic once and render it on different platforms. GPUI is GPU-native and macOS-first with Linux support in progress.

## Where the reactive model wins

GPUI's approach has its own strengths, and they show up most in larger applications.

Local reasoning is better. In a large iced app, your message enum grows. A lot. Every button, every text input, every timer produces a message variant. Your update function turns into a giant match statement. You can split it into sub-functions, but the routing still happens in one place. In GPUI, state mutations live next to the UI that triggers them. The click handler for a button sits right there in the view code. You can understand a component without reading a file of message definitions.

Performance is more granular. iced re-renders when state changes, and the granularity is the entire view function. GPUI tracks which entities each view accessed during rendering and only re-renders views that depend on changed entities. For a complex application with hundreds of components, this matters. Zed's code editor uses GPUI and re-renders at 120fps because only the parts that changed get redrawn.

Async is first-class. GPUI has a built-in async runtime integrated with the rendering cycle. You can spawn futures that resolve on the main thread and update entities when they complete. In iced, async work returns `Command<Message>`, which is idiomatic but means every async operation goes through the message dispatch.

```rust
// GPUI: async work updates the entity directly
entity.update(cx, |entity, cx| {
    let entity_handle = cx.handle();
    cx.spawn(async move |cx| {
        let data = fetch_from_server().await;
        entity_handle.update(&mut cx, |entity, cx| {
            entity.data = data;
            cx.notify();
        });
    })
    .detach();
});
```

There is also less message boilerplate. It adds up. In iced, every interaction needs a message variant. A form with ten fields needs at least ten variants. In GPUI, you update the field directly in the callback. Less ceremony, less noise.

## The scaling problem in both

Neither architecture scales perfectly. They just break in different ways.

In iced, the message enum becomes a bottleneck. A real application might have 50 to 200 message variants. Grouping them into sub-enums helps, but you still end up with a deep match tree. Refactoring state means updating message variants and match arms across the codebase. I have worked on iced apps where adding a single button required changes in four files.

In GPUI, the problem is implicit dependencies. When a view accesses five different entities during rendering, you might not realize it depends on all of them. Change one entity and the view re-renders even though it only needed the other four. This is a specific kind of over-rendering that is hard to track down because the dependency graph is implicit, built from access patterns rather than declared relationships.

For strategies on managing this in GPUI, see the [architecture guide](/docs/architecture/) and the [testing patterns](/blog/testing-gpui-applications/) post.

## So which should you use?

Use iced if:

- You value testability above all else
- You want cross-platform rendering including web
- Your app is small to medium and the message enum will not explode
- You like the Elm architecture from previous experience

Use GPUI if:

- Rendering performance matters for your use case
- Your app is complex enough that local reasoning beats central dispatch
- You want tight async integration without message wrapping
- You are building for macOS or Linux desktop and do not need web

Both frameworks are usable in production. Both have rough edges. The choice comes down to which tradeoffs match your application and your brain.

## Getting started

If you want to try GPUI without starting from zero, [gpui-starter](https://github.com/freeoxide/gpui-starter) is a production-ready boilerplate with multi-page navigation, theming, i18n, form validation, and more. Check the [getting started guide](/docs/getting-started/) to have a working app in under ten minutes.
