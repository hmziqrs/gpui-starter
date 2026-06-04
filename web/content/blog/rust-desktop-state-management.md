---
title: "State management patterns for Rust desktop applications"
description: "How to manage state in Rust desktop apps: entity pattern, global state, context system, and reactive updates with GPUI."
date: 2026-06-05
tags: [Rust, state, patterns]
draft: false
---

State management is where most desktop apps either click into place or become unmaintainable. In web land, you pick a library: Redux, Zustand, Signals, whatever. In Rust desktop apps built with GPUI, the runtime gives you three primitives and expects you to combine them well. Those primitives are `Entity<T>`, `Global`, and `Context<T>`. I have shipped production GPUI code with all three, and I want to walk through when each one fits, what the common traps are, and how to think about state in a desktop Rust app more broadly.

## Why state management matters in Rust desktop apps

Desktop apps are long-running processes. A user might keep your app open for hours or days. State accumulates. Windows open and close. Background tasks finish at unpredictable times. The file system changes out from under you.

Rust's ownership model helps here because the compiler catches data races and use-after-free at build time. But it does not tell you where to *put* your state. That is still an architecture decision. A bad one means you end up with tight coupling between views and data, God objects that hold everything, or callback spaghetti that makes async logic impossible to follow.

The [architecture guide](/docs/architecture/) for GPUI apps defines three tiers of state: hot, warm, and cold. I find this framework useful even outside GPUI, so I will recap it briefly before diving into the specific patterns.

## The entity pattern: owning state per component

`Entity<T>` is the workhorse. You create one with `cx.new()`, and you get back a handle to a Rust struct that GPUI tracks internally:

```rust
struct EditorState {
    content: String,
    cursor_position: usize,
    dirty: bool,
}

let state: Entity<EditorState> = cx.new(|_cx| EditorState {
    content: String::new(),
    cursor_position: 0,
    dirty: false,
});
```

Reading state is synchronous and borrow-checked:

```rust
let is_dirty = state.read(cx).dirty;
```

Mutating state goes through `update()`, which gives you exclusive access:

```rust
state.update(cx, |state, _cx| {
    state.content.push_str("hello");
    state.dirty = true;
});
```

One thing to understand: entities are reference-counted handles, not the data itself. Cloning an `Entity<T>` is cheap (it increments a ref count). The actual struct lives in GPUI's internal storage. When the last strong reference drops, the entity is deallocated.

### When to use entities

Use `Entity<T>` for state that belongs to a specific window or component. The text in an editor buffer. The selected row in a table. The open/closed state of a panel. If the state makes no sense without the thing that owns it, put it in an entity.

I wrote about this pattern in depth in the [entity system deep dive](/blog/gpui-entity-system-explained/). The short version: entities give you window-scoped, type-safe, thread-safe state without locking.

### The Vec<Entity<T>> trap

Here is a mistake I see often. You have a list of items, and you make each one an entity:

```rust
// This causes problems at scale
struct App {
    items: Vec<Entity<Item>>,
}
```

This works fine for 20 items. At 2,000 items you start noticing GC-like pauses because GPUI has to track every entity. At 50,000 items your app stutters. The fix: keep item data in a normalized store (a `HashMap<ItemId, Item>` or similar) and only create entities for the items currently visible on screen. I go into more detail on this in the [state management with entities post](/blog/state-management-gpui-entities/).

## Global state: the app-wide singleton

Not everything belongs to a window. Configuration, feature flags, shared service handles, database connections. These exist once and live for the entire lifetime of the app. GPUI handles this with the `Global` trait:

```rust
struct AppConfig {
    theme: String,
    language: String,
    api_endpoint: String,
}

impl Global for AppConfig {}
```

Once registered, any code with access to a context can read it:

```rust
let config = cx.global::<AppConfig>();
let theme = &config.theme;
```

Setting a global replaces the previous value:

```rust
cx.set_global(AppConfig {
    theme: "dark".into(),
    language: "en".into(),
    api_endpoint: "https://api.example.com".into(),
});
```

### When to use globals

Use globals for things that are truly app-scoped. A single database connection pool. A configuration object loaded at startup. An auth token. If you find yourself passing the same value through every function parameter or storing it in every entity, that is a sign it should be a global.

Be careful with mutable globals. There is no built-in mechanism to observe changes to a global. If you need reactivity when a global changes, consider wrapping it in an entity instead and subscribing to it. Globals are best for read-heavy, write-rare state.

## The context system: side effects and reactivity

`Context<T>` is not state storage. It is the control plane GPUI hands you when it calls into your code. You get different context types depending on scope: `App` at the top level, `Window` for window operations, `Context<T>` inside a component method.

The context is where reactive updates happen. When you mutate entity state inside `update()`, nothing re-renders until you call `cx.notify()`:

```rust
entity.update(cx, |state, cx| {
    state.count += 1;
    cx.notify(); // triggers re-render
});
```

This is explicit by design. GPUI does not diff a virtual DOM or track proxy mutations. You tell it when something changed. This avoids the "ghost re-render" problem where changing a field you do not even display causes the entire tree to reconcile.

The context also handles async work and subscriptions:

```rust
// Spawn a background task
entity.update(cx, |state, cx| {
    let entity_handle = cx.entity().downgrade();
    cx.spawn(async move |cx| {
        let data = fetch_from_server().await;
        cx.update(&entity_handle, |state, cx| {
            state.data = Some(data);
            cx.notify();
        })?;
    })
    .detach();
});
```

And subscribing to changes on another entity:

```rust
cx.subscribe(&other_entity, |this, other, event, cx| {
    this.update(cx, |state, cx| {
        state.reaction_to(event);
        cx.notify();
    });
});
```

## Putting it all together: a tiered approach

Here is how I structure state in a real GPUI desktop app.

**Hot state** (changes frequently, always visible): entity owned by the active window. Text buffers, form input, selected items. Created when the user opens the thing, dropped when they close it.

**Warm state** (medium churn, shared across windows): a normalized store, usually a `HashMap<K, V>` inside an entity. Think of it as a client-side database. All items are keyed by typed IDs. Components query into this store rather than holding their own copies.

**Cold state** (large, disk-backed): SQLite. Full document bodies, search indexes, cached API responses. Load on demand, keep only what fits comfortably in RAM. The [SQLite integration guide](/docs/getting-started/) covers this setup.

This tiering keeps memory usage predictable. Hot state is small by definition. Warm state is bounded because you normalize it. Cold state lives on disk and streams in as needed.

## Common mistakes I have made

**Putting everything in globals.** Early on I treated globals like a service locator. Every piece of shared state became a global. This made testing painful because globals persist between test cases. I had to manually call `cx.remove_global::<T>()` in teardown. Now I use globals only for true singletons and inject everything else through constructors or entity fields.

**Ignoring `WeakEntity<T>`.** Cycles happen. Entity A subscribes to entity B, which subscribes back to entity A. Both hold strong references. Neither gets dropped. Use `WeakEntity<T>` for subscription targets that should not keep the source alive:

```rust
let weak = entity.downgrade();
cx.subscribe(&weak.upgrade().unwrap(), |this, other, event, cx| {
    // handle event
});
```

**Not calling `cx.notify()`.** GPUI will not re-render if you forget. The mutation happens, the data is correct, but the UI shows stale values. This is the number one "bug" I debug in code reviews. Always call `notify()` after mutations that affect the view.

**Over-observing.** Every `cx.observe()` call adds overhead. If you observe ten entities from one component, that is ten subscription slots that fire on every change. Batch your observations or use a single aggregator entity that collects state from multiple sources.

## Reactive updates without a virtual DOM

GPUI's approach to reactivity is simple compared to web frameworks. There is no virtual DOM diff. No dependency tracking graph. No proxy objects that intercept property writes. You mutate state inside `update()`, call `cx.notify()`, and the component re-renders.

This means the performance model is straightforward. A re-render costs exactly what your `render()` method costs. If your render method is fast (and in GPUI it should be, since layout is GPU-accelerated), re-renders are cheap. If your render method does something expensive, that is your bottleneck and you can see it directly in a profiler.

The tradeoff is granularity. `cx.notify()` re-renders the entire component. You cannot opt into partial re-renders at the property level like you can with fine-grained reactive systems (Solid.js signals, for example). In practice this rarely matters because GPUI components are small and render is fast. But if you have a component rendering thousands of rows, you should split it into smaller components where only the changed part re-renders.

## State persistence across sessions

Desktop users expect their app to restore its state when they relaunch. Window positions, recent files, unsaved drafts. This means you need to serialize hot and warm state to disk periodically.

I use a simple approach: an entity that acts as a persistence coordinator. On a timer (or on specific events like "document saved"), it collects serializable state from entities and writes it to SQLite. On startup, it reads from SQLite and distributes state to the entities that need it.

```rust
struct PersistenceCoordinator {
    db: Entity<Database>,
}

impl PersistenceCoordinator {
    fn snapshot(&mut self, cx: &mut Context<Self>) {
        let snapshot = AppSnapshot::collect(cx);
        self.db.update(cx, |db, _cx| {
            db.save_snapshot(snapshot);
        });
    }
}
```

This keeps persistence logic in one place rather than scattered across every entity. Each entity stays focused on its own domain, and the coordinator handles the glue.

## Final thoughts

State management in Rust desktop apps is less about choosing the right library and more about understanding the three primitives your framework gives you. In GPUI: entities for window-scoped state, globals for app-wide singletons, context for side effects and reactivity. The hard part is discipline: keeping hot state small, normalizing warm state, and offloading cold state to disk.

If you want to see these patterns in a real project, [gpui-starter](https://github.com/hmziqrs/gpui-boilerplate) implements all three tiers with SQLite persistence, theme management as global state, and per-component entities for interactive state. The [getting started guide](/docs/getting-started/) walks through the full setup.
