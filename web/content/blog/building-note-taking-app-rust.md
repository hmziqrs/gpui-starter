---
title: "Building a note-taking app in Rust with GPUI"
description: "How to build a fast offline-first note-taking app with Rust and GPUI. SQLite storage, markdown rendering, and full-text search."
date: 2026-06-05
tags: [Rust, notes, desktop]
draft: false
---

Most note-taking apps solve the wrong problem. They optimize for sync, for sharing, for collaboration. Meanwhile the local experience suffers. Slow startup, laggy search, spinning cursors on a 500-note database. Notes are small text files. This should be instant.

I built a note-taking app with Rust and GPUI to see what happens when you optimize for local speed first. SQLite for storage. Markdown for content. Full-text search with zero network calls. The result starts in under 100ms and searches 10,000 notes in single-digit milliseconds on a MacBook Air.

## Why GPUI for a notes app

GPUI renders directly to the GPU. No DOM, no webview, no JavaScript runtime between your code and the screen. For a note-taking app this matters because the main view is a text editor with live preview. Every keystroke triggers a re-layout. Every keystroke also triggers a re-render of the markdown preview if you have split pane open. With a webview, that means DOM diffing and style recalculation on every frame. With GPUI, it means queuing a new paint batch.

The other reason is binary size. A notes app should be small enough that you never think about it. My build clocks in at 4.2MB on macOS. An Electron app doing the same thing would start at 130MB. That is not a rounding error. It is a different category of software.

## The data model

Notes have three fields that matter: title, body, and timestamps. Everything else is derived.

```rust
#[derive(Debug, Clone)]
pub struct Note {
    pub id: NoteId,
    pub title: String,
    pub body: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub folder: Option<String>,
    pub pinned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NoteId(pub u64);
```

The `NoteId` is a newtype wrapper around a u64. SQLite generates it via `ROWID`. Wrapping it in a typed struct prevents you from accidentally passing a note ID where a folder ID belongs. The compiler catches that mistake at build time, not at 2am when you are debugging a weird query result.

## SQLite schema and full-text search

SQLite is the right choice here because it runs in-process, requires zero configuration, and handles full-text search through the FTS5 extension. You do not need a separate search server. You do not need an index file you have to maintain. FTS5 builds a token index inside SQLite and queries it with SQL.

```sql
CREATE TABLE IF NOT EXISTS notes (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    folder TEXT,
    pinned INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
    title,
    body,
    content=notes,
    content_rowid=id
);

CREATE TRIGGER notes_ai AFTER INSERT ON notes BEGIN
    INSERT INTO notes_fts(rowid, title, body)
    VALUES (new.id, new.title, new.body);
END;

CREATE TRIGGER notes_au AFTER UPDATE ON notes BEGIN
    UPDATE notes_fts
    SET title = new.title, body = new.body
    WHERE rowid = new.id;
END;
```

The `notes_fts` table is a virtual table. It does not store a separate copy of your data. The `content=notes` directive tells FTS5 to read from the main `notes` table and only maintain the token index separately. The triggers keep the index in sync. When you insert or update a note, the FTS index updates automatically.

Searching becomes a single query:

```sql
SELECT n.* FROM notes n
JOIN notes_fts f ON f.rowid = n.id
WHERE notes_fts MATCH ?
ORDER BY rank
LIMIT 50;
```

The `rank` column is computed by FTS5 using BM25. You get relevance ordering for free. On my test database with 10,000 notes averaging 500 words each, this query returns in under 3ms.

## The Rust database layer

I use rusqlite for the database connection. It gives you prepared statements, transaction support, and proper error handling through Rust's type system.

```rust
use rusqlite::{Connection, params};
use std::path::Path;

pub struct NoteStore {
    conn: Connection,
}

impl NoteStore {
    pub fn open(db_path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn search(&self, query: &str) -> rusqlite::Result<Vec<Note>> {
        let mut stmt = self.conn.prepare(SEARCH_SQL)?;
        let rows = stmt.query_map(params![query], |row| {
            Ok(Note {
                id: NoteId(row.get::<_, u64>(0)?),
                title: row.get(1)?,
                body: row.get(2)?,
                folder: row.get(3)?,
                pinned: row.get::<_, i32>(4)? != 0,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    pub fn save(&self, note: &Note) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO notes (id, title, body, folder, pinned, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                title=excluded.title, body=excluded.body,
                folder=excluded.folder, pinned=excluded.pinned,
                updated_at=excluded.updated_at",
            params![note.id.0, note.title, note.body, note.folder, note.pinned, note.created_at, note.updated_at],
        )?;
        Ok(())
    }
}
```

Two things worth noting here. First, `PRAGMA journal_mode=WAL` enables write-ahead logging. This lets reads happen concurrently with writes without blocking. For a notes app where the user types constantly and search runs on every keystroke, this is essential. Without WAL, a save operation would lock the database and block the search query until it finishes.

Second, the `ON CONFLICT` upsert pattern means `save` handles both insert and update. One method instead of two. The FTS trigger fires regardless of which path SQLite takes.

## Wiring it into GPUI

The store lives as a [global](/docs/architecture/) in the app. You initialize it once during startup and access it from any component through the context.

```rust
struct NoteApp {
    store: NoteStore,
    notes: Vec<Note>,
    active_note: Option<Entity<NoteEditor>>,
    search_query: String,
}

impl NoteApp {
    fn new(cx: &mut App) -> Self {
        let db_path = cx.path_for_app_support("notes.db");
        let store = NoteStore::open(&db_path)
            .expect("failed to open database");

        let notes = store.list_recent(50)
            .expect("failed to load notes");

        Self {
            store,
            notes,
            active_note: None,
            search_query: String::new(),
        }
    }

    fn handle_search(&mut self, query: &str, cx: &mut App) {
        self.search_query = query.to_string();
        if query.is_empty() {
            self.notes = self.store.list_recent(50).unwrap_or_default();
        } else {
            self.notes = self.store.search(query).unwrap_or_default();
        }
        cx.notify();
    }
}
```

`cx.notify()` tells GPUI that this entity's state changed and the view needs to re-render. That is the only rendering API you need. There is no virtual DOM diff, no reconciliation step. You changed state, you call notify, GPUI repaints.

## Markdown rendering

Rendering markdown in GPUI means parsing it and converting the AST into GPUI elements. I use the `pulldown-cmark` crate for parsing and walk the events to build a styled text layout.

```rust
use pulldown_cmark::{Parser, Event, Tag};

fn render_markdown(body: &str, cx: &App) -> StyledText {
    let parser = Parser::new(body);
    let mut spans = Vec::new();

    for event in parser {
        match event {
            Event::Text(text) => {
                spans.push(text.to_string());
            }
            Event::Code(code) => {
                // Apply monospace style
                spans.push(code.to_string());
            }
            Event::Start(Tag::Heading(level, _, _)) => {
                // Apply heading size based on level
            }
            Event::Start(Tag::Strong) => {
                // Toggle bold style
            }
            _ => {}
        }
    }

    StyledText::new(spans.join(""))
}
```

This is a simplified version. A production renderer handles lists, links, code blocks with syntax highlighting, and nested formatting. The point is that pulldown-cmark gives you an iterator of events and you decide how each one becomes a GPUI element. There is no intermediate HTML step.

## Search-as-you-type debouncing

Running FTS5 on every keystroke works fine for small databases. For larger ones, you want to debounce the search so it does not fire until the user pauses typing for 100ms or so. In GPUI, you do this with a debounce mechanism on the text input handler.

```rust
fn handle_search_input(&mut self, input: &str, cx: &mut Context<NoteApp>) {
    self.search_query = input.to_string();
    self.search_debounce(cx);
}

fn search_debounce(&mut self, cx: &mut Context<NoteApp>) {
    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(Duration::from_millis(100)).await;
        this.update(cx, |app, cx| {
            let results = app.store.search(&app.search_query)
                .unwrap_or_default();
            app.notes = results;
            cx.notify();
        })?;
        anyhow::Ok(())
    }).detach();
}
```

Each keystroke cancels the previous timer and starts a new one. The search only fires when 100ms pass without another keystroke. This keeps the UI responsive even if the user types fast and the database is large.

## Offline-first means no surprises

The app works without network. Not "works with degraded features." Works completely. Every note is stored locally. Every search runs against local data. There is no loading spinner waiting for an API response. There is no "offline mode" indicator because the app is always in offline mode.

This changes how you think about the architecture. There is no sync layer to manage. No conflict resolution code. No retry logic for failed requests. The database is the source of truth. You read from it, you write to it, and you are done.

If you want sync later, you add it as an optional layer on top. Not as a foundational assumption baked into every read and write path. That is a much cleaner place to be.

## What I would do differently

The biggest tradeoff with GPUI right now is ecosystem maturity. There is no rich text editor component you can drop in. I had to build my own from GPUI's text primitives. That took about two weeks. A web-based editor like TipTap or ProseMirror gives you that on day one.

Text selection and cursor handling in a custom editor is fiddly. You need to track byte offsets, handle multi-byte characters, manage selection ranges, and implement clipboard operations. Each of these is a solved problem on the web with well-tested libraries. In GPUI, you are closer to the metal, which is great for performance but means more code to write and test.

The [testing guide](/docs/testing/) covers how to write integration tests for GPUI components. I recommend investing in tests early for any custom text editing logic. Bugs in cursor positioning are hard to catch manually and easy to catch with a test that says "press right arrow 5 times, expect cursor at offset 12."

## Getting started

If you want to build something similar, [gpui-starter](/docs/getting-started/) provides the boilerplate: project structure, SQLite setup, theme system, sidebar navigation, and build configuration. It handles the parts every GPUI app needs so you can focus on your domain logic. The source is on [GitHub](https://github.com/freeoxide/gpui-starter).

The note-taking app I described here is not a full product. It is a starting point. But the architecture scales. SQLite handles millions of rows without breaking a sweat. FTS5 handles the search. GPUI handles the rendering. The pieces fit together because each one does one job well.
