---
title: "Building a desktop chat application with Rust and GPUI"
description: "Architecture for a desktop chat app in Rust: WebSocket connections, message persistence with SQLite, and real-time UI updates using GPUI entities and context."
date: 2026-06-05
tags: [Rust, chat, desktop]
draft: false
---

Building a chat application sounds straightforward until you start counting the moving parts. You need persistent WebSocket connections that survive network changes. A local database that stores thousands of messages without dragging down the UI. Real-time rendering that scrolls smoothly while new messages arrive. And all of this on a desktop where users expect native performance, not the sluggishness of an Electron wrapper.

I recently went through this exercise using Rust and GPUI. Here is what the architecture looks like, with the tradeoffs I hit along the way.

## The three layers

A chat app splits into three distinct concerns: the network layer (WebSocket connections, reconnection, message serialization), the persistence layer (storing messages, threading, search), and the UI layer (rendering message lists, input handling, real-time updates). Each layer has different performance characteristics and different failure modes. The trick is keeping them independent enough that a WebSocket disconnect does not freeze the UI, and a slow database query does not block rendering.

The [architecture docs](/docs/docs/architecture/) describe a similar split for any GPUI application: hot state in entities, warm state in normalized stores, cold state on disk. A chat app follows this pattern naturally.

## WebSocket connections with reconnection

The network layer needs to handle three things: connecting, reconnecting when the connection drops, and buffering outbound messages while disconnected. Tokio-tungstenite handles the WebSocket protocol. The reconnection logic sits on top.

```rust
pub struct ReconnectPolicy {
    pub max_retries: u8,
    pub base_delay_ms: u64,
    pub max_delay_ms: Option<u64>,
}

impl ReconnectPolicy {
    pub fn delay_for_attempt(&self, attempt: u8) -> Duration {
        let exp = 1u64.checked_shl(attempt as u32).unwrap_or(u64::MAX);
        let raw = self.base_delay_ms.saturating_mul(exp);
        let capped = self.max_delay_ms.map_or(raw, |cap| raw.min(cap));
        Duration::from_millis(capped)
    }
}
```

The default policy starts at 500ms and doubles each attempt, capped at 30 seconds. After 5 failed attempts, the client stops trying and surfaces an error to the user. This matters because infinite reconnection loops drain battery on laptops and confuse users who think the app is working when it is not.

The client itself uses a split stream pattern. The WebSocket stream gets divided into a write half (stored behind `Arc<Mutex<Option<WriteHalf>>>`) and a read half that gets consumed in a loop:

```rust
pub struct WebSocketClient {
    pub url: String,
    pub state: ConnectionState,
    pub reconnect: ReconnectPolicy,
    pub on_message: Option<MessageHandler>,
    inner: Arc<Mutex<Option<WriteHalf>>>,
    pending: Arc<Mutex<Vec<String>>>,
}
```

Messages sent while disconnected go into a bounded pending buffer (64 messages max). When the connection comes back, the buffer drains into the write half before the read loop starts. This means short network blips are invisible to the user. The tradeoff is that long disconnections drop messages once the buffer fills. For a chat app, that is the right call. You cannot buffer forever without running out of memory.

### Wiring into GPUI context

GPUI runs async work through `cx.spawn()`, which gives you a handle to the GPUI async context. The WebSocket connect loop runs as a background task:

```rust
pub fn spawn_connect(url: String, cx: &mut gpui::App) {
    let mut client = WebSocketClient::new(url);
    cx.spawn(async move |cx| {
        cx.background_executor()
            .spawn(async move {
                if let Err(e) = client.connect_loop().await {
                    tracing::error!(error = %e, "websocket terminated");
                }
            })
            .await
    })
    .detach();
}
```

The important part here is `cx.background_executor().spawn(...)`. The heavy I/O work (WebSocket reads and writes) runs on a background executor, not on the main GPUI thread. The outer `cx.spawn()` gives you access to the GPUI context for posting results back to the UI.

## Message persistence with SQLite

Every chat message needs to survive app restarts. SQLite is the obvious choice for a desktop app: single-file database, no server process, reads in microseconds because there is no network hop. The [SQLite tutorial](/blog/sqlite-rust-desktop-tutorial/) covers the setup in detail, but here is the schema I ended up with:

```sql
CREATE TABLE IF NOT EXISTS messages (
    id          TEXT PRIMARY KEY,
    channel_id  TEXT NOT NULL,
    sender_id   TEXT NOT NULL,
    body        TEXT NOT NULL,
    timestamp   INTEGER NOT NULL,
    edited_at   INTEGER,
    FOREIGN KEY (channel_id) REFERENCES channels(id)
);

CREATE INDEX IF NOT EXISTS idx_messages_channel_time
    ON messages(channel_id, timestamp);
```

The composite index on `(channel_id, timestamp)` matters a lot. Chat apps query messages by channel, ordered by time, almost exclusively. Without this index, loading a channel with 10,000 messages triggers a full table scan. With it, the query completes in under a millisecond.

The storage wrapper uses `Arc<Mutex<Connection>>` because `rusqlite::Connection` is `Send` but not `Sync`. Multiple async tasks can acquire the mutex, but only one writes at a time:

```rust
pub struct MessageStore {
    conn: Arc<Mutex<Connection>>,
}

impl MessageStore {
    pub fn load_messages(
        &self,
        channel_id: &str,
        limit: usize,
        before: Option<i64>,
    ) -> Result<Vec<Message>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        // Query messages with cursor-based pagination
        // ...
    }

    pub fn insert(&self, msg: &Message) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO messages (id, channel_id, sender_id, body, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![&msg.id, &msg.channel_id, &msg.sender_id, &msg.body, &msg.timestamp],
        )?;
        Ok(())
    }
}
```

One thing I got wrong initially: I was holding the mutex lock while parsing query results into structs. The lock should be released as soon as the raw rows are collected. Parsing is CPU work that does not need the database connection held hostage. In practice this matters when you load a channel with thousands of messages on a slow machine.

### WAL mode

Enable WAL mode on startup. Without it, SQLite uses rollback journaling, which blocks readers during writes. WAL mode allows concurrent reads and writes, which is exactly what a chat app needs: the UI thread reads messages to render while incoming WebSocket messages get written in the background.

```rust
conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
```

`synchronous=NORMAL` is safe with WAL mode and avoids the fsync on every write that `FULL` mode imposes. The difference is noticeable: message inserts drop from ~2ms to ~0.1ms on a typical SSD.

## Real-time UI updates

The UI layer is where things get interesting. A chat window has three dynamic regions: the message list, the input area, and a status indicator showing connection state. All three update independently at different rates.

In GPUI, state lives in entities. A `ChatView` entity holds the current channel's messages, the scroll position, and the connection state:

```rust
struct ChatView {
    messages: Vec<ChatMessage>,
    channel_id: String,
    input_text: String,
    connection_state: ConnectionState,
    auto_scroll: bool,
}
```

When a new message arrives from the WebSocket, the message gets persisted to SQLite first, then the entity updates:

```rust
// Inside a cx.spawn block:
let msg = parse_message(&text)?;
message_store.insert(&msg)?;

chat_view.update(cx, |view, cx| {
    view.messages.push(msg);
    if view.auto_scroll {
        // Scroll to bottom
    }
    cx.notify();
});
```

`cx.notify()` tells GPUI that this entity has changed and needs to re-render. GPUI batches these notifications within a single frame, so if 5 messages arrive in the same 16ms window, the render only happens once.

### Avoiding the scroll jump

The single most annoying bug in chat UIs is the scroll jumping to the bottom when you are reading older messages. The fix is simple: only auto-scroll when the user is already at the bottom. Detect "at bottom" by checking if the scroll offset is within a threshold of the maximum:

```rust
fn is_near_bottom(scroll_offset: f32, content_height: f32, viewport_height: f32) -> bool {
    let distance_from_bottom = content_height - viewport_height - scroll_offset;
    distance_from_bottom < 40.0 // 40px threshold
}
```

When a message arrives and `is_near_bottom` is false, the message still gets added to the Vec, but the scroll position stays put. A subtle badge or "new messages" indicator tells the user something arrived. Without this check, users get pulled away from whatever they were reading every time someone sends a message.

## Handling backpressure

A busy channel can receive hundreds of messages per second. Rendering each one individually kills frame rate. The solution is to batch incoming messages and render at most once per frame:

```rust
struct ChatView {
    messages: Vec<ChatMessage>,
    pending: Vec<ChatMessage>, // New messages waiting for next render
}

impl Render for ChatView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.pending.is_empty() {
            self.messages.extend(self.pending.drain(..));
        }
        // Render self.messages
    }
}
```

Incoming WebSocket messages get pushed to `pending` via `entity.update()`. The render method drains `pending` into `messages` once per frame. This caps rendering work to once per 16ms regardless of message frequency.

For very large channels (10,000+ messages), you need virtualization: only render the messages visible in the viewport plus a small overscan buffer. GPUI handles this naturally if you structure your data correctly, but the implementation depends on your specific layout. The [state management guide](/blog/state-management-gpui-entities/) covers the entity patterns that make this work at scale.

## The offline problem

Desktop apps go offline. Laptops sleep and wake in different network environments. Users close lids on trains. The chat app needs to handle this gracefully.

My approach: treat offline as a first-class state. The `ConnectionState` enum includes `Reconnecting { attempt: u8 }` which the UI renders as a banner with the current retry count. When the connection comes back, the client sends a "catch-up" request to the server with the timestamp of the last received message. The server responds with all messages since that timestamp.

This means the local SQLite database is the source of truth for what the user sees. The server is the source of truth for what actually happened. The client reconciles on reconnect. Messages sent while offline get queued in the pending buffer and flushed when the connection resumes.

## What I would do differently

Two things. First, I would have started with cursor-based pagination instead of offset-based. Offset pagination degrades on large tables because SQLite has to scan past all preceding rows. Cursor-based (using the last message timestamp as a cursor) is constant-time regardless of position.

Second, I would have decoupled the WebSocket message handler from the entity update sooner. Early on, I had the message handler directly calling `entity.update()`. When parsing or database writes were slow, the WebSocket read loop blocked. Moving to a channel-based queue (the handler sends parsed messages through a `tokio::sync::mpsc`, a separate task drains the queue and updates the entity) eliminated that bottleneck entirely.

## Getting started

If you want to build something like this, the [getting started guide](/docs/docs/getting-started/) walks through setting up a GPUI project from scratch. The [gpui-starter repo](https://github.com/hmziqrs/gpui-boilerplate) includes a working WebSocket scaffold with reconnection, SQLite persistence with migrations and WAL mode, and the entity patterns for real-time updates described in this post. Clone it, run `cargo run`, and start building.
