---
title: "Async patterns for Rust desktop applications"
description: "Handling async operations in Rust GUI apps: spawning tasks, updating state, backpressure, and cancellation."
date: 2026-06-05
tags: [Rust, async, patterns]
draft: false
---

Async Rust is one of those things that clicks once you have used it in a real project. The runtime handles concurrency. The borrow checker keeps data races out. The type system forces you to think about error paths upfront. But when you bring async into a GUI application, the patterns change. You are no longer writing a server that processes requests and returns responses. You are writing a program that owns a main thread, renders 60 frames per second, and needs to show the user something useful while network requests are still in flight.

I have spent the last year building desktop apps with Rust and GPUI. These are the async patterns I keep coming back to, along with the mistakes that taught me why they matter.

## Spawning work off the main thread

The first rule of async in a GUI app: never block the render loop. GPUI renders on the main thread. If you run a network request or a database query synchronously in a render callback, the frame pipeline stalls. The user sees a frozen window. On macOS, the spinning beachball appears within seconds.

GPUI provides two layers of async execution. `cx.spawn()` creates a task that has access to the GPUI context, meaning you can update entities and trigger re-renders from inside it. `cx.background_executor().spawn()` creates a task on a background thread pool that has no access to GPUI state. The common pattern is nesting them:

```rust
cx.spawn(async move |cx| {
    cx.background_executor()
        .spawn(async move {
            // Heavy I/O here: network, filesystem, database
            fetch_data_from_server().await
        })
        .await
})
.detach();
```

The outer `cx.spawn()` gives you the GPUI async context. The inner `background_executor().spawn()` moves the actual work off the main thread. When the inner task completes, control returns to the outer task, which can then update UI state.

The `.detach()` call means "I do not care about the return value, just run this to completion." If you need to cancel the task later, store the `Task` handle instead of detaching.

## Updating UI state from async tasks

Once your background work finishes, you need to write the result into the UI. GPUI uses entities for mutable state. Updating an entity from an async context looks like this:

```rust
cx.spawn(async move |cx| {
    let data = cx.background_executor()
        .spawn(async move { fetch_data().await })
        .await;

    // Back on the GPUI context, update the entity
    my_entity.update(cx, |entity, cx| {
        entity.data = data;
        cx.notify();
    });
})
.detach();
```

`entity.update()` acquires mutable access to the entity and runs the closure. `cx.notify()` tells GPUI that this entity has changed and needs to re-render. GPUI batches notifications within a single frame, so if five entities call `notify()` in the same 16ms window, the render pass happens once, not five times.

One mistake I made early: calling `entity.update()` from inside the `background_executor` task. This does not compile because the background executor has no access to the GPUI context. The update has to happen in the outer `cx.spawn()` task, after the background work completes. This separation is intentional. It prevents you from mutating UI state from a random thread.

## Cancellation that actually works

Cancellation in async Rust is cooperative. There is no thread preemption. A task stops only when it reaches an `.await` point and the runtime decides to cancel it. For short tasks this is fine. For long-running work, like a file download or a WebSocket connection loop, you need to check a cancellation signal yourself.

The simplest approach is an `AtomicBool`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

// In your long-running task:
loop {
    if SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
        // Clean up and exit
        return;
    }
    // Do one unit of work
    process_next_item().await;
}
```

This works well for app-wide shutdown. When the user closes the window, set the flag, and every background task checks it on its next iteration. The flag is lock-free, so there is no contention overhead.

For per-task cancellation, use GPUI's `Task` handle. When you store the task instead of detaching it, dropping the handle cancels the task:

```rust
struct MyView {
    active_fetch: Option<Task<()>>,
}

impl MyView {
    fn start_fetch(&mut self, cx: &mut Context<Self>) {
        self.active_fetch = Some(cx.spawn(async move |this, cx| {
            let data = cx.background_executor()
                .spawn(async move { fetch_data().await })
                .await;
            this.update(cx, |view, cx| {
                view.data = Some(data);
                cx.notify();
            });
        }));
    }

    fn cancel_fetch(&mut self) {
        // Dropping the Task handle cancels it
        self.active_fetch = None;
    }
}
```

When the `MyView` entity is dropped (for example, the user navigates away from the page), the `active_fetch` field is dropped too, and the spawned task cancels at its next `.await` point. No dangling callbacks updating a dead entity.

## Graceful shutdown on quit

Desktop apps need to shut down cleanly. If you kill the process while a database write is in progress, you can corrupt data. The shutdown sequence I use has two phases: cooperative drain, then forced cancellation.

```rust
fn drain_with_timeout(timeout: Duration, cx: &mut App) -> Task<()> {
    request_shutdown(); // Sets the AtomicBool

    cx.spawn(async move |cx| {
        let start = Instant::now();
        loop {
            let active = cx.update(|cx| active_task_count(cx));
            if active == 0 {
                return; // All tasks finished cooperatively
            }
            if start.elapsed() >= timeout {
                cx.update(|cx| force_cancel_remaining(cx));
                return;
            }
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
        }
    })
}
```

First, set the shutdown flag so cooperative tasks start winding down. Then poll every 100ms to check if they finished. If the timeout expires (I use 3 seconds), force-cancel everything that is still running. This gives well-behaved tasks a chance to finish their current unit of work while guaranteeing the app eventually exits.

## Backpressure and batching

A fast network connection can deliver data faster than the UI can render it. If you update an entity on every incoming message, you end up calling `cx.notify()` hundreds of times per second. Each notification triggers a render pass. The frame budget blows up.

The fix is batching. Accumulate incoming data in a separate buffer and drain it once per frame:

```rust
struct MessageView {
    messages: Vec<Message>,
    pending: Vec<Message>, // Incoming messages waiting for next render
}

impl Render for MessageView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.pending.is_empty() {
            self.messages.extend(self.pending.drain(..));
        }
        // Render self.messages
    }
}
```

Incoming messages get pushed to `pending` via `entity.update()`. The `render()` method drains `pending` into `messages` once per frame. This caps rendering work at 60Hz regardless of the incoming message rate. The tradeoff is a small latency increase: messages appear one frame later than they would with immediate rendering. At 60fps that is 16ms, which users cannot perceive.

For high-volume scenarios, you can also drop messages. If the `pending` buffer exceeds a threshold, discard the oldest entries. This sounds harsh, but a desktop app that stutters is worse than a desktop app that skips intermediate data points. The [state management patterns](/blog/gpui-entity-system-explained/) in GPUI entities make this simple since all mutation goes through `update()`.

## Debouncing rapid events

Window resize events fire dozens of times per second while the user drags a resize handle. If each resize triggers a config save to disk, you waste I/O and potentially conflict with yourself. The fix is debouncing: wait until the events stop, then act once.

```rust
struct RootView {
    pending_bounds_flush: Option<Task<()>>,
}

// In the resize handler:
this.pending_bounds_flush = Some(cx.spawn(async move |this, cx| {
    cx.background_executor()
        .timer(Duration::from_millis(500))
        .await;

    let _ = this.update(cx, |this, cx| {
        if let Some(bounds) = this.pending_bounds.take() {
            save_window_bounds(bounds, cx);
        }
    });
}));
```

Each resize event replaces the previous `Task`, cancelling it. The timer starts fresh. Only after 500ms of no resize events does the save actually run. One disk write instead of fifty. This shows up everywhere: search-as-you-type, auto-save, live validation.

## Error handling in async chains

Async tasks fail. Network requests time out. Database writes get rejected. The user goes offline. You need to handle these errors without crashing the app or silently swallowing them.

The pattern I prefer: handle errors inside the async task and update the entity with a result type, not an error propagation chain.

```rust
cx.spawn(async move |this, cx| {
    let result = cx.background_executor()
        .spawn(async move { fetch_user_profile().await })
        .await;

    this.update(cx, |view, cx| {
        match result {
            Ok(profile) => {
                view.profile = Some(profile);
                view.error = None;
            }
            Err(e) => {
                view.error = Some(e.to_string());
                // Keep the old profile data visible
            }
        }
        cx.notify();
    });
})
.detach();
```

The entity always gets updated, whether the task succeeded or failed. The UI can show the error inline next to the data instead of showing a blank screen or a modal alert. This matches what users expect from a desktop app: something useful is always visible, even when parts of it are stale or failed.

## Selecting between async operations

Sometimes you need to race two async operations against each other. For example, showing cached data immediately while fetching fresh data in the background. Or timing out a slow request. Tokio's `select!` macro handles this, but in GPUI the pattern looks a bit different because you work with the background executor:

```rust
cx.spawn(async move |this, cx| {
    let fetch = cx.background_executor()
        .spawn(async move { fetch_latest().await });

    let timeout = cx.background_executor()
        .timer(Duration::from_secs(5));

    tokio::select! {
        result = fetch => {
            this.update(cx, |view, cx| {
                view.handle_result(result);
                cx.notify();
            });
        }
        _ = timeout => {
            this.update(cx, |view, cx| {
                view.error = Some("Request timed out".into());
                cx.notify();
            });
        }
    }
})
.detach();
```

`select!` cancels the losing branch automatically. If the fetch completes in 2 seconds, the timeout timer gets dropped. If the timeout fires first, the fetch task gets cancelled. This is simpler than manually tracking elapsed time.

## Getting started

These patterns are drawn from real code in [gpui-starter](https://github.com/hmziqrs/gpui-boilerplate), a boilerplate for Rust desktop apps built on GPUI. It includes working examples of all the async patterns described here: task spawning with cancellation, graceful shutdown with cooperative drain, background HTTP requests with backpressure, and debounced config saves. The [getting started guide](/docs/getting-started/) walks through setting up your first project.
