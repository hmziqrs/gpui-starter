---
title: "The GPUI entity system explained: state and reactivity"
description: "How GPUIs entity system works: Model, View, Entity, Context. State management, observers, and thread safety explained with real Rust code."
date: 2026-06-05
tags: [GPUI, entities, deep-dive]
draft: false
---

If you have written a GPUI component, you have used `Entity<T>`. It is the type that shows up in every struct field, every method signature, every `cx.new()` call. But the entity system is more than a smart pointer. It is the core of how GPUI handles state, scheduling, and reactivity, all without a virtual DOM or a diffing algorithm.

This post covers what entities actually are, how reads and updates work, why weak references matter, and how the observer system ties it all together. I will assume you have written at least a "hello world" GPUI app, maybe followed the [getting started guide](/docs/getting-started/). If you want a broader overview of state management options, see the [state management with entities post](/blog/state-management-gpui-entities/).

## What is an Entity, exactly?

An `Entity<T>` is a handle to a Rust struct `T` that GPUI tracks internally. It is not the struct itself. The struct lives in GPUI's internal storage, and the `Entity<T>` handle gives you controlled access to it.

```rust
let counter: Entity<Counter> = cx.new(|_cx| Counter { count: 0 });
```

`cx.new()` allocates the `Counter` inside GPUI's runtime, registers it, and returns a strong handle. The handle is cheap to clone because it is just a reference-counted pointer. The actual `Counter` struct lives exactly once.

You read from an entity with `read()`:

```rust
let count: usize = counter.read(cx).count;
```

And you mutate with `update()`:

```rust
counter.update(cx, |state, cx| {
    state.count += 1;
    cx.notify();
});
```

That `cx.notify()` call is the reactivity trigger. It tells GPUI "this entity changed, re-render anything that depends on it." There is no diffing, no virtual DOM comparison. You tell GPUI when something changed, and it handles the rest.

## The three entity flavors

GPUI gives you three related types for different ownership situations.

**`Entity<T>`** is a strong reference. Cloning it increments the reference count. As long as at least one `Entity<T>` exists, the underlying data stays alive. Use this as the default.

**`WeakEntity<T>`** is a weak reference. It does not keep the data alive. All operations on `WeakEntity` return `Result` because the underlying entity might have been dropped. Create one with `entity.downgrade()`.

**`AnyEntity`** is a type-erased handle. You use it when you need to store entities of different types in the same collection. Downcast back to a concrete type with `any_entity.downcast::<T>()`.

Most of the time you work with `Entity<T>`. You reach for `WeakEntity<T>` in two specific situations, which I will cover next.

## Why weak references matter

This is the single most common bug source in GPUI apps, so I want to spend time on it.

When you spawn an async task or store a callback, you need a reference to an entity inside the closure. If you capture a strong `Entity<T>`, you have created a retain cycle: the entity holds the closure (directly or indirectly), and the closure holds the entity. Neither can be freed.

```rust
// This creates a retain cycle
fn fetch_data(&mut self, cx: &mut Context<Self>) {
    let entity = cx.entity(); // strong reference

    cx.spawn(async move |cx| {
        let data = fetch_from_api().await;
        entity.update(cx, |state, cx| {
            state.data = Some(data);
            cx.notify();
        });
    }).detach();
}
```

If the user closes the window while the request is in flight, the entity stays alive because the spawned task still holds it. That is a memory leak, and worse, the `.update()` call might panic or behave unexpectedly depending on the window state.

The fix is always the same: downgrade to a weak reference.

```rust
fn fetch_data(&mut self, cx: &mut Context<Self>) {
    let weak = cx.entity().downgrade();

    cx.spawn(async move |cx| {
        let data = fetch_from_api().await;
        let _ = weak.update(cx, |state, cx| {
            state.data = Some(data);
            cx.notify();
        });
    }).detach();
}
```

The `let _ =` discards the `Result`. If the entity was dropped before the async task finished, the update is silently skipped. No panic, no leak.

The same pattern applies to parent-child relationships. Children should hold `WeakEntity<Parent>`, not `Entity<Parent>`.

## The inner context rule

Here is the second most common mistake. When you call `entity.update(cx, |state, inner_cx| { ... })`, the closure receives its own `inner_cx`. You must use that, not the outer `cx`.

```rust
// Wrong: using outer cx inside the closure
counter.update(cx, |state, inner_cx| {
    cx.notify(); // compile error or runtime panic
});

// Correct: use inner_cx
counter.update(cx, |state, inner_cx| {
    inner_cx.notify();
});
```

GPUI's borrow checker enforces this at runtime. The outer `cx` is already borrowed by the `update()` call. Using it again triggers a double-borrow panic. The `inner_cx` is the context scoped to this specific update, and it is the only safe handle.

## Observers and subscriptions

Entities are most useful when they talk to each other. GPUI provides two mechanisms: `observe` and `subscribe`.

**`cx.observe()`** watches an entity for `cx.notify()` calls. When the observed entity calls `notify`, your callback runs.

```rust
cx.observe(&settings_entity, |this, settings, cx| {
    let theme = settings.read(cx).theme.clone();
    this.apply_theme(theme);
    cx.notify();
}).detach();
```

The `.detach()` call makes the subscription permanent. Without it, the subscription would be dropped when the enclosing scope ends.

**`cx.subscribe()`** watches for typed events. The observed entity calls `cx.emit(Event)` and you handle it.

```rust
// In the emitter
impl DataStore {
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.data = load_from_disk();
        cx.emit(DataEvent::Reloaded);
        cx.notify();
    }
}

// In the subscriber
cx.subscribe(&data_store, |this, store, event: &DataEvent, cx| {
    match event {
        DataEvent::Reloaded => {
            this.refresh_view();
            cx.notify();
        }
    }
}).detach();
```

The distinction matters. Use `observe` when you just need to know "something changed." Use `subscribe` when you need to know *what* changed. Observers are simpler. Subscriptions carry more information. Pick the one that fits.

One warning: do not create mutual observation. If entity A observes entity B, and entity B observes entity A, you can get infinite notification loops. Design your data flow in one direction.

## The update boundary

GPUI processes updates sequentially on the main thread. When you call `entity.update()`, GPUI acquires a lock on that entity's state, runs your closure, and releases it. This means two things.

First, you cannot nest updates on the same entity. This will panic:

```rust
counter.update(cx, |state, cx| {
    counter.update(cx, |state2, cx| {
        // panic: already borrowed
    });
});
```

Second, updates on different entities should be sequential, not nested:

```rust
// Sequential: fine
entity_a.update(cx, |state, cx| { state.value = 1; cx.notify(); });
entity_b.update(cx, |state, cx| { state.value = 2; cx.notify(); });

// Nested: risky, may panic depending on entity relationships
entity_a.update(cx, |_, cx| {
    entity_b.update(cx, |_, cx| {
        // may work, may panic
    });
});
```

Keep your update closures short. Do the mutation, call `notify` if something visual changed, and get out. If you need to coordinate multiple entities, update them sequentially at the call site.

## Batch your notify calls

Every `cx.notify()` schedules a re-render. If you are updating three fields on the same entity, call `notify` once at the end, not three times.

```rust
fn update_profile(&mut self, name: String, email: String, bio: String, cx: &mut Context<Self>) {
    self.name = name;
    self.email = email;
    self.bio = bio;
    cx.notify(); // one render, not three
}
```

You can go further and skip `notify` entirely when nothing actually changed:

```rust
fn set_count(&mut self, new_count: usize, cx: &mut Context<Self>) {
    if self.count != new_count {
        self.count = new_count;
        cx.notify();
    }
}
```

This is a small optimization for a single component, but it compounds. A list view with 200 items that all re-render on every keystroke will feel sluggish. Conditional `notify` calls prevent that.

## Async: spawn vs background_spawn

GPUI has two async entry points. Understanding the difference prevents subtle bugs.

`cx.spawn()` runs on the UI thread. Use it for short async operations where you need to update entities afterward. The closure receives a `WeakEntity<T>` and an `AsyncApp` context.

`cx.background_spawn()` runs on a background thread pool. Use it for CPU-heavy work: image processing, parsing large files, cryptography. You cannot update entities from inside a background spawn. You need to chain back to the foreground.

```rust
fn process_images(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
    let weak = cx.entity().downgrade();

    let result = cx.background_spawn(async move {
        // Heavy work off the UI thread
        paths.iter().map(|p| generate_thumbnail(p)).collect::<Vec<_>>()
    });

    // Chain back to foreground for entity updates
    result.then(cx.spawn(move |thumbnails, cx| {
        let _ = weak.update(cx, |state, cx| {
            state.thumbnails = thumbnails;
            state.processing = false;
            cx.notify();
        });
    })).detach();
}
```

The rule of thumb: if the async work touches the network or disk but is I/O bound, `spawn` is fine. If it is CPU bound and could take more than a few milliseconds, use `background_spawn`.

## Putting it together

Here is a realistic component that uses most of these patterns together: a data loader that fetches from an API, shows loading state, handles errors, and notifies a parent.

```rust
struct DataLoader {
    loading: bool,
    data: Option<Vec<Item>>,
    error: Option<String>,
    parent: WeakEntity<ParentComponent>,
}

impl DataLoader {
    fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        cx.notify();

        let weak = cx.entity().downgrade();

        cx.spawn(async move |cx| {
            match fetch_items().await {
                Ok(items) => {
                    let _ = weak.update(cx, |state, cx| {
                        state.loading = false;
                        state.data = Some(items);
                        state.error = None;
                        cx.notify();
                    });
                    // Notify parent
                    let _ = weak.update(cx, |state, cx| {
                        let _ = state.parent.update(cx, |parent, cx| {
                            parent.on_data_loaded();
                            cx.notify();
                        });
                    });
                }
                Err(e) => {
                    let _ = weak.update(cx, |state, cx| {
                        state.loading = false;
                        state.error = Some(e.to_string());
                        cx.notify();
                    });
                }
            }
        }).detach();
    }
}
```

Notice the patterns: weak references in async closures, inner `cx` usage, single `notify` per state change, error handling on both the fetch and the entity update.

## The mental model

Think of entities as cells in a spreadsheet. Each cell holds a value. When a cell changes, it notifies its dependents. The `Entity<T>` handle is the cell reference. `WeakEntity<T>` is a reference that might point to a deleted row. `cx.observe()` is like a formula that watches another cell. `cx.subscribe()` is like a macro that runs on specific events.

The constraint: all updates happen on the main thread, one at a time. There are no race conditions because there is no concurrency on the write path. You trade parallelism for simplicity, and for a desktop UI framework, that is the right trade.

If you want to see these patterns in a real app with navigation, themes, i18n, and async data loading, check out [gpui-starter on GitHub](https://github.com/freeoxide/gpui-starter) or read the [getting started docs](/docs/getting-started/). The entity patterns described here are the same ones used throughout the codebase.
