---
title: Performance
description: How to prevent render loops, eliminate idle CPU usage, and keep GPUI apps responsive. Nine patterns for avoiding the most common performance traps.
---

GPUI apps tend to be fast until they are not. The framework handles layout, painting, and GPU batching for you. The performance problems I see in production GPUI code all come from the same root cause: code in `render()` that schedules more renders. This page documents the nine patterns I use to keep frame times low and CPU idle when the user is not doing anything.

If you want to understand why GPUI works this way, the [rendering pipeline deep dive](/blog/gpui-rendering-pipeline-deep-dive/) explains the frame lifecycle. For the broader state model that these patterns build on, see the [architecture guide](/docs/architecture/) and the [entity system explained](/blog/gpui-entity-system-explained/).

## Core rule: render() is a pure projection

`render()` must read state and return elements. Nothing else. Any mutation inside `render()` that triggers `cx.notify()` creates a feedback loop. The app re-renders, hits the same mutation, re-renders again, and never stops.

```rust
// BAD: entity.update() inside render schedules another render
fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.table.update(cx, |state, cx| {
        state.set_rows(self.compute_rows());
        state.refresh(cx); // cx.notify() → render → cx.notify() → ...
    });
    div()
}

// BETTER: dirty flag fires once per data change, not every frame
// This is a narrow exception for one-time initialization.
// Prefer pushing data from the event handler itself (see Pattern 1).
fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    if self.rows_dirty {
        self.rows_dirty = false; // cleared BEFORE update so re-render won't re-enter
        self.table.update(cx, |state, cx| {
            state.set_rows(self.cached_rows.clone());
            state.refresh(cx);
        });
    }
    div()
}
```

The prohibited list inside `render()` is broader than just `entity.update()`. All of these can trigger handlers that schedule another render:

- `set_value` on an input or similar widget
- `cx.notify()`
- `cx.subscribe()` or `cx.observe()` (called every frame, they accumulate subscriptions without bound)
- `cx.spawn()`
- Any entity `update()` that emits events

The `cx.subscribe()` and `cx.observe()` case is easy to miss because the code looks harmless. But if you call them inside `render()`, you are registering a new subscription on every frame. After a few hundred frames you have hundreds of redundant callbacks, each one potentially calling `cx.notify()`.

## Pattern 1: Dirty flags for one-time data push

The cleanest approach is to push data from the event handler that caused the change. No render involvement at all.

```rust
fn on_data_received(&mut self, data: Data, cx: &mut Context<Self>) {
    self.rows = build_rows(&data);
    self.table.update(cx, |t, cx| {
        t.set_rows(self.rows.clone());
        t.refresh(cx);
    });
    cx.notify();
}
```

When that is not feasible (for instance, the data must be computed from render context), use a dirty flag. Set the flag in the event handler. Clear it in render before the push.

```rust
struct MyView {
    data_dirty: bool,
    table: Entity<TableState>,
}

// In render: narrow exception; flag cleared before update so re-renders won't re-enter
if std::mem::take(&mut self.data_dirty) {
    self.table.update(cx, |t, cx| {
        t.set_rows(self.rows.clone());
        t.refresh(cx);
    });
}

// In the event handler
fn on_data_received(&mut self, data: Data, cx: &mut Context<Self>) {
    self.rows = build_rows(&data);
    self.data_dirty = true;
    cx.notify();
}
```

The important detail: `std::mem::take` reads and clears in one step. Clearing before the update means a re-render triggered by the update will not re-enter the block.

## Pattern 2: Guard every notify() against no-op changes

Every `cx.notify()` schedules a full re-render. Call it only when state actually changed.

```rust
// BAD: re-renders even when value is identical
fn on_something(&mut self, new_val: String, cx: &mut Context<Self>) {
    self.value = new_val;
    cx.notify();
}

// GOOD
fn on_something(&mut self, new_val: String, cx: &mut Context<Self>) {
    if self.value != new_val {
        self.value = new_val;
        cx.notify();
    }
}
```

The same rule applies to `cx.observe()` callbacks that reload external data:

```rust
cx.observe(&session, |this, _, cx| {
    let new_catalog = load_catalog();
    if this.catalog != new_catalog {   // equality guard
        this.catalog = new_catalog;
        cx.notify();
    }
});
```

This pattern is especially important in views that observe entities from the [architecture guide's](/docs/architecture/) hot state tier. A keystroke in a text input can trigger dozens of entity notifications per second. Without guards, each one becomes a full re-render with all the work that entails.

## Pattern 3: Bidirectional sync must be event-driven

When two UI elements stay in sync (for example, a URL bar and a params editor), each direction must live in its own event handler. Never centralize sync in `render()`.

```rust
// BAD: sync in render creates: render → set_value → Change → notify → render → ...
fn render(&mut self, w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.sync_inputs_from_model(w, cx); // calls set_value → emits Change → cx.notify()
    div()
}

// GOOD: each direction handled in its own subscription
fn new(cx: &mut Context<Self>) -> Self {
    // Direction 1: url_input → params editor
    cx.subscribe_in(&url_input, cx, |this, _, event, w, cx| {
        if let InputEvent::Change(val) = event {
            this.sync_params_from_url(&val, w, cx);
        }
    });
    // Direction 2: params editor → url_input (guarded)
    cx.subscribe_in(&params_editor, cx, |this, _, event, w, cx| {
        if let ParamsEvent::Changed = event {
            let new_url = this.build_url_from_params();
            if this.model.url != new_url {
                this.url_input.update(cx, |s, cx| s.set_value(new_url.clone(), w, cx));
                this.model.url = new_url;
            }
        }
    });
    // Initial population via dirty flag, not in render
    Self { draft_dirty: true, ... }
}
```

### ReentrancyGuard is a safety net, not a fix

A guard can suppress immediate re-entrancy during a one-time init sync. But if the root cause (sync running in render) is not removed, the deferred `cx.notify()` the guard emits fires on the next frame and restarts the cycle. Fix the root cause first. Use the guard as a last resort.

## Pattern 4: Subscription cleanup on row rebuild

When you rebuild a list of child entities (for example, KV editor rows), clear the old subscriptions first. Dropped entity handles become no-ops, but the `Subscription` objects still live in memory.

```rust
struct MyView {
    rows: Vec<RowEntity>,
    row_subs: Vec<Subscription>, // separate from long-lived subscriptions
}

fn rebuild_rows(&mut self, data: &[Row], cx: &mut Context<Self>) {
    self.row_subs.clear(); // drop old Subscription objects first
    self.rows = data.iter().map(|row| {
        let input = cx.new(|cx| InputState::new(cx, row.value.clone()));
        self.row_subs.push(cx.subscribe_in(&input, cx, Self::on_row_changed));
        input
    }).collect();
}
```

Without the `clear()`, each rebuild adds 2N new subscriptions that never go away. After ten rebuilds of a 20-row editor, you have 400 active subscriptions pointing at entities that no longer exist in the view. This is a common cause of slow memory growth in long-running sessions. For more on this class of problem, see the [memory management patterns](/blog/rust-desktop-memory-management/) post.

## Pattern 5: Observe precisely, not broadly

Observe the entity that owns the data you care about. Observing a wide entity (such as a parent view) and doing expensive work on every notification is the fastest way to create per-keystroke SQLite queries.

```rust
// BAD: AppRoot observes every RequestTabView, reloads catalog on each keystroke
cx.observe(&request_tab, |this, _, cx| {
    this.catalog = load_workspace_catalog(); // 5 SQLite queries per keypress
    cx.notify();
});

// GOOD: reload catalog only when the workspace tree actually mutated
fn on_request_saved(&mut self, ...) {
    self.catalog = load_workspace_catalog();
    cx.notify();
}
// Incidental RequestTabView notifications (typing, focus) never touch the catalog
```

If you must observe broadly, gate on a revision or identity field:

```rust
cx.observe(&request_tab, |this, tab, cx| {
    let revision = tab.read(cx).revision();
    if this.last_revision == revision { return; }
    this.last_revision = revision;
    this.catalog = load_workspace_catalog();
    cx.notify();
});
```

This ties into the hot/warm/cold state model from the [architecture guide](/docs/architecture/). If the data you are reloading is warm state (catalogs, indexes, metadata), you should reload it only when the source of truth actually changes, not on every frame.

## Pattern 6: Cache external side effects in render

External calls inside `render()` bypass GPUI's change detection and run every frame. WebView loads, file reads, D-Bus calls: all of these execute unconditionally.

```rust
// BAD: repaints WKWebView even when HTML is identical
fn render_preview(&mut self, cx: &mut Context<Self>) -> Div {
    self.webview.update(cx, |w, _| w.load_html(&self.html)); // every render
    div()
}

// GOOD: cache and compare
fn render_preview(&mut self, cx: &mut Context<Self>) -> Div {
    if self.last_html.as_deref() != Some(&self.html) {
        self.last_html = Some(self.html.clone());
        self.webview.update(cx, |w, _| w.load_html(&self.html));
    }
    div()
}
```

The same principle applies to file watchers. Debounce the callback and check whether the loaded content actually differs from the currently applied value before notifying observers.

## Pattern 7: Explicit async task lifecycle

Dropping a `Task<T>` handle cancels the task silently. Either store it on the owning entity (for long-lived operations) or call `.detach()` (for true fire-and-forget). Never let a task fall out of scope by accident.

```rust
struct MyView {
    inflight: Option<Task<()>>, // stored → cancels when reassigned or entity drops
}

fn start_op(&mut self, cx: &mut Context<Self>) {
    self.inflight = Some(cx.spawn(async move { /* ... */ }));
    // detach for true fire-and-forget:
    // cx.spawn(async move { /* ... */ }).detach();
}
```

When consuming a channel in a spawned task, break on entity drop and guard with an `operation_id` to discard stale results from cancelled operations:

```rust
while let Some(event) = rx.recv().await {
    if let Err(_) = entity.update(cx, |this, cx| {
        if this.active_id != operation_id { return; } // stale operation guard
        this.process(event, cx);
    }) {
        break; // entity dropped: stop consuming the channel
    }
}
```

For a deeper treatment of async patterns in GPUI including backpressure and cancellation strategies, see the [async patterns guide](/blog/rust-desktop-async-patterns/).

## Pattern 8: Batch notify for high-throughput streams

Per-message `cx.notify()` in a WebSocket or streaming HTTP response causes UI invalidation on every incoming frame. At high throughput this saturates the render loop.

```rust
// BAD: notify on every message
while let Some(msg) = stream.next().await {
    entity.update(cx, |this, cx| {
        this.messages.push(msg);
        cx.notify(); // fires 100s of times/sec under load
    }).ok();
}

// GOOD: batch into a ring buffer, notify on flush cadence
let (tx, mut rx) = mpsc::channel::<Message>(256);

// Flush task: drain up to N messages, then notify once
cx.spawn(async move {
    loop {
        let msg = rx.recv().await?;
        entity.update(cx, |this, _| this.buffer.push(msg)).ok();
        // drain any already-queued messages without extra awaits
        while let Ok(msg) = rx.try_recv() {
            entity.update(cx, |this, _| this.buffer.push(msg)).ok();
        }
        entity.update(cx, |_, cx| cx.notify()).ok(); // one notify per batch
    }
}).detach();
```

Also keep the visible message buffer bounded. Use a ring buffer, not a `Vec`, to prevent RSS growth during long sessions. This is the same class of leak described in the [memory management patterns](/blog/rust-desktop-memory-management/) post: a 1MB/hour leak becomes 168MB after a week.

## Pattern 9: Avoid entity reentrancy

Do not re-enter a mutable update on the same entity from within its own active update path. GPUI will panic or silently no-op depending on context. Defer follow-up work instead.

```rust
// BAD: calls entity.update() on self from within self's update closure
fn do_work(&mut self, cx: &mut Context<Self>) {
    self.helper(cx); // if helper calls cx.update on Self, that's re-entrant
}

// GOOD: schedule follow-up via cx.notify() or a spawned task
fn do_work(&mut self, cx: &mut Context<Self>) {
    self.state = next_state;
    cx.notify(); // render will observe the new state; no re-entrant update needed
}
```

If you find yourself needing to call `entity.update()` from inside another `entity.update()` on the same entity, restructure so the state change happens directly and the render picks it up. The [state management guide](/blog/state-management-gpui-entities/) covers entity access patterns in more detail.

## Checklist

Run through this before shipping a new view or subscriber. I keep a printed copy at my desk because these are easy to forget under deadline pressure.

- `render()` contains no `entity.update()`, `cx.notify()`, `cx.subscribe()`, `cx.observe()`, `cx.spawn()`, or external I/O
- Every `cx.notify()` is inside an `if value_changed` guard
- Bidirectional sync uses event handlers, not render; no `ReentrancyGuard` as a substitute fix
- Row-rebuild helpers call `subscriptions.clear()` before creating new rows
- Observers read only from the narrowest entity that carries the changed data
- External side effects (webview, file, DB) are cached and compared before re-applying
- High-throughput streams batch into a bounded buffer and call `cx.notify()` once per flush
- Every `Task<T>` is either stored on an entity field or explicitly `.detach()`ed
- Async tasks `break` on entity-drop error and check `operation_id` for staleness
- No re-entrant mutable updates on the same entity within a single update path
