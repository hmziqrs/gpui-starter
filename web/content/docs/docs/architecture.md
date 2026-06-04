---
title: Architecture
description: GPUI architecture patterns for entity state management, async boundaries, and scale-ready app design in Rust desktop apps
---

This page covers the architecture decisions baked into gpui-starter. These are patterns I settled on after hitting real problems: entities growing without bound, async updates landing on dropped windows, streams saturating the render loop. Each section shows what went wrong and what the fix looks like in code.

If you are new to the framework, start with [Getting Started](/docs/getting-started/) for project layout, then come back here for the model underneath.

## GPUI API facts

A handful of GPUI API details catch people off guard. These are the ones that cause the most subtle bugs.

### Global trait bound

```rust
// CORRECT
pub trait Global: 'static {}

// WRONG: Send + Sync are not required
pub trait Global: 'static + Send + Sync {}
```

`set_global` replaces and `remove_global` removes a global. They are not permanent for the app lifetime. Tests can clear globals between runs, which matters for [testing entity logic](/docs/testing/) with fresh state.

### Subscription::detach()

`detach()` consumes the `Subscription` and returns `()`. It cannot be stored:

```rust
// WRONG: detach() returns (), not Subscription
let s: Subscription = cx.subscribe(&entity, handler).detach();

// CORRECT: store the Subscription handle if you need to cancel it later
let s: Subscription = cx.subscribe(&entity, handler);
self.subs.push(s); // dropped = cancelled

// CORRECT: detach if it should live forever
cx.subscribe(&entity, handler).detach();
```

### observe_global bounds differ by call site

```rust
App::observe_global::<G: Global>(...)        // requires Global marker trait
Context<T>::observe_global::<G: 'static>(...) // only requires 'static
```

### WeakEntity in async tasks

Use `WeakEntity<T>` by default in async closures that may outlive UI entities. Strong `Entity<T>` is valid only in short-lived scoped flows where ownership cycles cannot form. See [section 9](#fallible-async) for the full async error handling pattern.

## State tiers

Use a three-tier model. Do not flatten everything into a single entity or a single global:

| Tier | Churn | Mechanism | Examples |
|------|-------|-----------|----------|
| Hot | Rapid, visible on screen | `Entity<T>` owned by a window | Active editor draft, in-flight request state, selected list row |
| Warm | Medium, potentially large cardinality | Normalized value store with typed IDs | Item catalogs, history index rows, environment metadata |
| Cold | Large or archived, disk-backed | SQLite + blob files | Response bodies, stream logs, full history |

The point: materialize `Entity<T>` wrappers only for items the user is actively editing. Keep the rest as plain values keyed by ID. This keeps entity count bounded and avoids the ownership traps covered in the next section.

## Ownership policy

Do not keep `Vec<Entity<Item>>` as the primary long-lived store for any catalog or list.

```rust
// WRONG: every collection gets an entity regardless of whether it's active
struct AppState {
    collections: Vec<Entity<Collection>>,
}

// CORRECT: catalog is a value type; entity created only for the active editor
struct AppState {
    catalog: CollectionCatalog,          // value type, ID-keyed
    active_editor: Option<Entity<CollectionEditor>>,
}
```

The wrong version allocates a GPUI entity per collection, holds subscriptions open for all of them, and makes it easy to accidentally observe the wrong layer. The [routing and navigation](/docs/routing/) system in gpui-starter follows this pattern: the `Page` enum is a plain value, and entities are created only for the active page's view.

## Memory budgets

Define explicit caps and enforce them. Do not let response bodies, stream buffers, or history lists grow without bound.

```rust
pub struct ResponseBudgets;
impl ResponseBudgets {
    pub const PREVIEW_CAP: usize = 2 * 1024 * 1024;   // 2 MiB in-memory preview
    pub const PER_TAB_CAP: usize = 32 * 1024 * 1024;  // 32 MiB per active tab
}

pub enum BodyRef {
    Empty,
    InMemory { bytes: Vec<u8>, truncated: bool },
    DiskBlob { id: String, preview: Option<Vec<u8>>, size: u64 },
}
```

When a cap is exceeded: keep only the preview in memory, spill the full payload to a blob store, and mark the UI as truncated with a "load from disk" action.

For streaming message buffers (WebSocket, gRPC): use a fixed-size ring buffer, not a `Vec`. Keep aggregate counters (`total_received`, `dropped_count`) separately so you can show the user that data was discarded without storing all of it.

## Streaming and backpressure

Never call `cx.notify()` per incoming message at high throughput. It saturates the render loop.

```rust
// WRONG: one notify per message
while let Some(msg) = stream.next().await {
    entity.update(cx, |this, cx| {
        this.buffer.push(msg);
        cx.notify(); // fires hundreds of times/sec
    }).ok();
}

// CORRECT: drain available messages, then notify once
cx.spawn(async move {
    loop {
        let msg = rx.recv().await?;
        entity.update(cx, |this, _| this.buffer.push_back(msg)).ok();
        while let Ok(msg) = rx.try_recv() {
            entity.update(cx, |this, _| this.buffer.push_back(msg)).ok();
        }
        entity.update(cx, |_, cx| cx.notify()).ok(); // one notify per batch
    }
}).detach();
```

Use bounded channels between the network reader and the UI flush task. On sustained overflow: drop oldest visible messages, increment a dropped counter, and degrade gracefully. The [performance guide](/docs/performance/) covers this same pattern from the render-loop side, including how to keep the visible buffer bounded.

## Cancellation model

Every in-flight operation needs four things:

1. **Operation ID**: lets late responses identify and discard stale results
2. **Task handle**: stored on the owning entity; dropping it cancels the task
3. **Protocol cancellation primitive**: abort handle/token that propagates to the network layer
4. **Lifecycle FSM**: states must be mutually exclusive

```rust
enum OpState {
    Idle,
    Sending,
    Waiting,    // request sent, awaiting response headers
    Receiving,  // streaming response body
    Completed,
    Failed(Error),
    Cancelled,
}
```

Guard against stale results after cancellation by checking the operation ID, not entity liveness:

```rust
while let Some(event) = rx.recv().await {
    if let Err(_) = entity.update(cx, |this, cx| {
        if this.active_op_id != operation_id { return; } // stale: discard
        this.process(event, cx);
    }) {
        break; // entity dropped
    }
}
```

A common mistake: checking `entity.is_alive()` instead of the operation ID. The entity might still be alive from a previous operation while the current one was cancelled. The ID comparison is the only reliable staleness check.

## Task lifecycle

Every `Task<T>` handle must be explicitly accounted for. Dropping it silently cancels the task.

```rust
struct MyView {
    inflight: Option<Task<()>>,  // reassigning cancels the previous task
}

fn start(&mut self, cx: &mut Context<Self>) {
    // Long-lived: store the handle
    self.inflight = Some(cx.spawn(async move { /* ... */ }));

    // Fire-and-forget: explicit detach
    cx.spawn(async move { /* ... */ }).detach();
}
```

The danger is accidental drops: a task stored in a local variable that goes out of scope at the end of the function. Always ask: who owns this task? If nobody, call `.detach()`. If an entity owns it, store it as a field. This is covered in more depth in the [async task hygiene section of the performance guide](/docs/performance/#pattern-7-async-task-hygiene).

## Persistence

Use SQLite (WAL mode) for structured data and a blob store for large payloads.

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
```

Required: schema versioning with migrations, crash-safe writes, and a compaction/cleanup policy.

Keep large bodies (response payloads, stream transcripts) out of SQLite rows: reference them by blob ID instead. This keeps the database lean and queries fast. The [secure storage](/docs/secure-storage/) module follows the same separation principle: opaque references in SQLite, actual values elsewhere (the OS keyring, in that case).

## Fallible async

All async operations that touch app, window, or entity state are fallible by design. A dropped app, window, or entity is a normal shutdown path, not an error.

```rust
cx.spawn(async move |mut cx| {
    let result = do_work().await;
    // update() returns Err if the entity was dropped: handle it, don't unwrap
    cx.update(|this, cx| {
        this.on_result(result, cx);
    }).ok(); // .ok() is intentional: drop is expected on shutdown
})
```

Never rely on `.unwrap()` or `.expect()` after async boundaries for entity/window/app updates. The entity can be dropped between `do_work().await` and the `cx.update()` call. This is not edge-case behavior; it happens on every window close during a background fetch.

## Secrets

Secrets must never land in the SQLite database or blob store. Store them in the platform credential store (macOS Keychain, Linux Secret Service, Windows Credential Manager).

```rust
// WRONG: secret value in the database
INSERT INTO environments (key, value) VALUES ('API_TOKEN', 'sk-...');

// CORRECT: opaque reference in the database, value in the keyring
INSERT INTO secret_refs (id, keyring_key) VALUES (?, ?);
// actual value lives in the OS keyring under `keyring_key`
```

Export/import flows must redact or rebind secrets explicitly. Logging must never include raw secret values. The [secure storage docs](/docs/secure-storage/) walk through the full keyring integration with the `keyring` crate.

## Virtualization

Large list-based UI surfaces must use virtualization. This is not optional for:

- History and activity lists
- Collection, folder, and request trees with large node counts
- Stream and message viewers

"Large" means the rendering cost grows linearly with item count. Without virtualization, scrolling and selection degrade at a few hundred items and OOM is possible at thousands. Validate with a performance test under target maximum dataset size.

## Entity reentrancy

Do not re-enter a mutable update on the same entity from within its own active update path.

```rust
// WRONG: nested update on the same entity
entity.update(cx, |this, cx| {
    this.helper(cx); // if helper calls entity.update() on the same entity -> panic/no-op
});

// CORRECT: mutate state and let the next render or a new update handle follow-up
entity.update(cx, |this, cx| {
    this.state = next_state;
    cx.notify();
});
```

Include regression tests for reentrancy and lifecycle race conditions in entity update flows. The [testing guide](/docs/testing/) shows how to write deterministic tests for state machines using `#[gpui::test]`.

## Render purity

See [Performance](/docs/performance/) for the full treatment. Short version:

`render()` must not call `set_value`, `cx.notify()`, `cx.subscribe()`, `cx.observe()`, `cx.spawn()`, or any entity `update()` that emits events. Any of these can schedule another render, creating a feedback loop.

## Acceptance checklist

Before shipping a new view or subscriber:

- [ ] No `Vec<Entity<_>>` as a primary long-lived catalog or list store
- [ ] Response and payload memory caps defined and enforced; large bodies spill to disk
- [ ] Stream buffers are bounded ring buffers, not `Vec`
- [ ] High-throughput streams batch-notify, not per-message notify
- [ ] Every operation has an ID, a stored `Task` handle, and a lifecycle FSM
- [ ] SQLite in WAL mode with migrations; large payloads in a blob store
- [ ] No `.unwrap()` or `.expect()` after async entity/window/app update boundaries
- [ ] Secrets stored in platform credential store; only opaque references in the DB
- [ ] Large list UIs use virtualization; validated by a performance test
- [ ] No re-entrant mutable updates on the same entity
- [ ] `render()` is a pure projection (see the [performance checklist](/docs/performance/#checklist))
