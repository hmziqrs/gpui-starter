---
title: "From prototype to production: scaling a GPUI app"
description: "The architectural decisions that turn a working GPUI prototype into a production desktop app: state tiers, memory budgets, cancellation, and persistence."
date: 2026-05-31
tags: [GPUI, Rust, architecture]
draft: false
---

"If it compiles, it probably works" is a nice slogan. It's not a deployment strategy.

Prototypes lie. Everything lives in one file, state is flat, errors get `unwrap()`d, and the whole thing runs fine with three items in a list. Then real users show up with real data, and the cracks open fast.

This post covers the specific architectural decisions that separate a GPUI prototype from something you can ship: state tiers, memory budgets, backpressure, cancellation, and persistence. I learned most of these the hard way.

## What breaks at scale

A typical GPUI prototype has a single `AppState` struct holding every piece of UI state as a field. Errors get `.unwrap()`d. Async operations spawn tasks that nobody tracks. The entire app lives in two or three files, and `render()` methods trigger side effects because it's convenient.

This works at demo scale. It falls apart when you add streaming data, large lists, persistent storage, and multiple concurrent operations. The symptoms are predictable: CPU spikes at idle, memory growing without bound, stale results overwriting fresh ones, and renders that trigger other renders in an infinite loop.

The fix isn't more code. It's different structure.

## The state tier model

Not all state is equal. GPUI apps work best when you split state into tiers based on how often it changes and how much of it exists.

Hot state is the stuff on screen right now: the active editor draft, the selected row, the in-flight request. This lives in `Entity<T>` owned by the window. It changes constantly and re-renders on every mutation.

Warm state is the catalog: hundreds or thousands of items that exist but aren't all visible. Item lists, history indexes, environment metadata. This should be normalized value types keyed by ID, not a `Vec<Entity<Thing>>`. You create entities only for the item being actively edited.

Cold state is the archive: response bodies, stream logs, full history. This lives on disk in SQLite and blob files. You load it on demand.

```rust
// Flat state that breaks at scale
struct AppState {
    collections: Vec<Entity<Collection>>,
    responses: Vec<Response>,
    settings: Settings,
    active: Option<Entity<Editor>>,
}

// Tiered state that scales
struct AppState {
    catalog: CollectionCatalog,           // warm: ID-keyed values
    active_editor: Option<Entity<Editor>>, // hot: only the active item
}
// Cold: SQLite + blob store, loaded on demand
```

The rule: don't materialize an entity for every item in your data set. Entities are for the thing the user is interacting with right now. Everything else is a value.

If you want the full reference for state tiers and ownership policies, the [architecture guide](/docs/architecture/) covers them with an acceptance checklist.

## Memory budgets

Unbounded growth is the most common production bug I've seen in desktop apps. Response payloads, stream buffers, history lists. They grow until the OS kills the process.

Define explicit caps. Enforce them.

```rust
pub struct ResponseBudgets;
impl ResponseBudgets {
    pub const PREVIEW_CAP: usize = 2 * 1024 * 1024;   // 2 MiB in memory
    pub const PER_TAB_CAP: usize = 32 * 1024 * 1024;  // 32 MiB per active tab
}

pub enum BodyRef {
    Empty,
    InMemory { bytes: Vec<u8>, truncated: bool },
    DiskBlob { id: String, preview: Option<Vec<u8>>, size: u64 },
}
```

When a payload exceeds the cap, keep a small preview in memory and spill the rest to a blob file on disk. The UI shows a truncated view with a "load full response" action. For streaming message buffers, use a fixed-size ring buffer instead of a `Vec`. Track `total_received` and `dropped_count` as separate counters.

This is the kind of thing nobody bothers with in a prototype. You should bother with it before your first production release, because users will find a way to feed your app a 500 MiB payload within a week.

## Streaming and backpressure

The most common performance mistake in GPUI apps: calling `cx.notify()` on every incoming message in a WebSocket or streaming HTTP response. At high throughput, this saturates the render loop. The UI freezes while the framework tries to re-render hundreds of times per second.

The fix is batch-flush. Drain all available messages from the channel, then notify once.

```rust
cx.spawn(async move {
    loop {
        let msg = rx.recv().await?;
        entity.update(cx, |this, _| this.buffer.push_back(msg)).ok();
        while let Ok(msg) = rx.try_recv() {
            entity.update(cx, |this, _| this.buffer.push_back(msg)).ok();
        }
        entity.update(cx, |_, cx| cx.notify()).ok();
    }
}).detach();
```

The bounded channel between the network reader and the UI flush task provides backpressure. If the UI can't keep up, the network reader blocks. On sustained overflow, drop the oldest messages, increment a counter, and degrade the UI gracefully.

For more on this pattern and other rendering optimizations, see the [performance guide](/docs/performance/) and the post on [GPUI rendering pipeline internals](/blog/gpui-rendering-pipeline-deep-dive/).

## The cancellation model

Every in-flight operation needs four things: an operation ID, a stored task handle, a cancellation primitive that reaches the network layer, and a lifecycle state machine.

The operation ID lets you discard stale results. When a user clicks "send" twice in quick succession, the first response arrives after the second request is already in flight. Without an ID check, the stale response overwrites the current one.

```rust
enum OpState {
    Idle,
    Sending,
    Waiting,
    Receiving,
    Completed,
    Failed(Error),
    Cancelled,
}

struct RequestOp {
    id: u64,
    state: OpState,
    handle: Option<Task<()>>,
}

// In the async consumer:
if this.active_op_id != operation_id { return; } // stale, discard
```

The state machine prevents invalid transitions. You can't go from `Cancelled` to `Receiving`. You can't start a new operation without cleaning up the old one. The task handle lives on the owning entity; dropping it cancels the task. This is the only reliable way to cancel work in GPUI.

## Persistence

Use SQLite with WAL mode for structured data. The configuration is specific and non-negotiable:

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
```

WAL mode allows concurrent reads while writes are happening. `synchronous = NORMAL` is safe with WAL and much faster than `FULL`. The busy timeout prevents "database is locked" errors under concurrent access.

Keep large payloads out of SQLite rows. Store them as blob files on disk and reference them by ID. This keeps the database lean and queries fast. You need schema versioning and migrations from day one, even if the schema is trivial. Adding columns to a production database without migrations is how you lose user data.

The [SQLite setup tutorial](/blog/sqlite-rust-desktop-tutorial/) walks through the full configuration with migration tooling.

## Render purity

The `render()` method must read state and return elements. Nothing else.

This is the rule that bites hardest in production. Calling `cx.notify()`, `cx.subscribe()`, `cx.spawn()`, `entity.update()`, or any method that emits events inside `render()` creates a feedback loop. The render triggers a notification, which triggers another render, which triggers another notification. CPU usage spikes and the app becomes unresponsive.

Every `cx.notify()` call should sit behind a guard that checks whether state actually changed. Bidirectional sync between UI elements must happen in event handlers, not in render. External side effects like webview loads or file reads must be cached and compared before re-applying.

## Secrets

Never store secrets in SQLite. Not encrypted, not encoded, not "temporarily." The database file sits on disk and is accessible to anything with filesystem access. Store secrets in the platform credential store: macOS Keychain, Linux Secret Service, Windows Credential Manager.

```rust
// Database stores only an opaque reference
INSERT INTO secret_refs (id, keyring_key) VALUES (?, ?);
// The actual value lives in the OS keyring under keyring_key
```

Export and import flows must redact secrets explicitly. Logging must never include raw secret values. This isn't a suggestion. It's a hard line.

The [secure storage docs](/docs/secure-storage/) cover the keyring integration in detail.

## What gpui-starter handles

gpui-starter sets up the boilerplate that every GPUI app needs: window setup, navigation, theme system with 21 built-in themes, i18n, a command launcher (Cmd+K), and a project structure that separates concerns by default. You get a working app in `cargo run` that already follows the patterns described here.

What it doesn't give you is the domain-specific architecture. The state tiers, memory budgets, cancellation model, and persistence layer are things you design based on what your app actually does. gpui-starter gives you a clean foundation. The scaling decisions are yours.

Ship the prototype. Then refactor the state, add the budgets, wire up the cancellation, and test with real data before you ship it to users. The people who stick around are the ones who notice when the app doesn't freeze.
