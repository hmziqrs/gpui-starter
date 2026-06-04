---
title: "Testing GPUI applications"
description: "Practical strategies for testing Rust desktop apps built with GPUI, from plain unit tests and the gpui test harness to hand-written fakes and VisualTestContext."
date: 2026-05-11
tags: [Rust, GPUI, testing]
draft: false
---

"If it compiles, it probably works" is a fun Rust saying. It is not a testing strategy. GPUI apps have a testing problem that web developers never face: there is no DOM. You cannot call `querySelector`, assert on CSS classes, or fire synthetic click events at an HTML element. The UI lives on the GPU. So how do you test it?

You test what matters and skip what doesn't. GPUI's architecture makes this more practical than you might expect. This post covers the testing approach I settled on while building gpui-starter, from plain `#[test]` functions through the GPUI test harness to integration tests with real rendering.

## What you can test without rendering

Most of the logic in a well-structured GPUI app lives outside `render()`. State, routing, config migrations, undo history, event queues. None of these need a window or a GPU context. They are plain Rust structs and functions, and you test them with plain `#[test]`.

The routing module in gpui-starter parses deep links like `gpui-starter://settings/notifications` into an `AppRoute` enum. The test for this is a regular Rust test with no GPUI machinery (see the [routing docs](/docs/routing/) for how the route system works):

```rust
#[test]
fn parses_supported_deep_links() {
    let home = AppRoute::parse_deep_link("gpui-starter://home").unwrap();
    assert_eq!(home, AppRoute::Page(Page::Home));

    assert_eq!(
        AppRoute::parse_deep_link("gpui-starter://settings/notifications").unwrap(),
        AppRoute::SettingsNotifications
    );
    assert!(AppRoute::parse_deep_link("https://example.com").is_err());
}
```

Same story for config migrations, undo stack logic, and event ordering. The `undo_stack` module has an internal `UndoModel` struct that handles push, pop, and rejection. Its tests exercise state transitions directly:

```rust
#[test]
fn record_clears_redo_history() {
    let mut model = UndoModel {
        future: vec![sample_entry()],
        ..UndoModel::default()
    };
    model.record(sample_entry());
    assert_eq!(model.past.len(), 1);
    assert!(model.future.is_empty());
}
```

These tests run fast, compose well, and catch real bugs. The config migration tests in gpui-starter caught a regression where a legacy config with `version: 0` would fail to enable the global shortcut flag. That is a behavioral bug, not a rendering bug, and you do not need a GPU to find it.

## When you need the GPUI test harness

Some code depends on GPUI's context system: entities, globals, subscriptions, async tasks. For that, GPUI provides `#[gpui::test]` and `TestAppContext`.

`TestAppContext` is a single-threaded, deterministic executor. You get entity creation, global state, and async task scheduling without opening a window. If your test needs a window (for focus handling, action dispatch, or actual rendering), you promote to `VisualTestContext`.

```rust
#[gpui::test]
fn test_entity_round_trip(cx: &mut TestAppContext) {
    let counter = cx.new(|_cx| Counter { count: 0 });

    counter.update(cx, |this, cx| {
        this.count += 1;
        cx.notify();
    });

    let value = counter.read_with(cx, |this, _| this.count);
    assert_eq!(value, 1);
}
```

For async operations, the test executor gives you `cx.run_until_parked()`, which advances all pending tasks to completion. Timer-based code, background fetches, detached tasks all complete deterministically. No flaky `sleep(100)` calls.

To enable GPUI tests in your project, add a feature flag:

```toml
[features]
test-support = ["gpui/test-support"]
```

Then run with `cargo test --features test-support`.

## Hand-written fakes over mocking libraries

gpui-starter includes a `src/testing.rs` module with fake implementations of external services: telemetry, connectivity, notifications, and secure storage. These are not mocks in the mocking-framework sense. They are hand-written fakes with real state and real behavior.

```rust
#[derive(Default)]
pub struct FakeNotificationBackend {
    pub sent: VecDeque<String>,
    pub fail_send: bool,
}

impl FakeNotificationBackend {
    pub fn send(&mut self, title: &str) -> Result<(), &'static str> {
        if self.fail_send {
            return Err("send failed");
        }
        self.sent.push_back(title.to_string());
        Ok(())
    }
}
```

This fake tracks every notification sent, and you can flip `fail_send` to test error paths. The tests for it verify both success and failure in the same test:

```rust
#[test]
fn fake_notification_backend_success_and_failure() {
    let mut backend = FakeNotificationBackend::default();
    backend.send("hello").expect("send");
    assert_eq!(backend.sent.len(), 1);

    backend.fail_send = true;
    assert!(backend.send("world").is_err());
    assert_eq!(backend.sent.len(), 1); // no new entry on failure
}
```

Why hand-written fakes instead of a mocking library? Rust's type system makes fakes cheap to write, and they compose better. A `FakeSecureStorage` that round-trips values through `set`/`get`/`delete` is more useful than a mock that verifies `set` was called with the right argument. You test behavior, not call counts.

## Testing globals and state transitions

GPUI globals are test-friendly by design. You call `set_global`, `update` it, then `read_with` to assert. Since `set_global` replaces the previous value, tests can reset state between runs without cleanup hooks.

The event queue in gpui-starter uses a `Global` to accumulate events, then drains them. Testing this with `TestAppContext` means you create the global, emit events, and assert the queue contents. No window required.

For state machines like `TaskStatus` (Queued, Running, Succeeded, Failed, Cancelled), test each transition explicitly. The `tasks` module has a `mutate_task` helper that finds a task by ID, applies a mutation, and re-emits the global. Testing that `succeed` sets progress to 100% and clears the error field is a few lines of setup with a `TestAppContext`.

## Form validation, command handling, and actions

Form validation in GPUI is logic. A field validator is a function that takes a string and returns `Result<(), ValidationError>`. Test it like any other pure function. The [forms docs](/docs/forms/) show how validators compose.

Command handling maps to GPUI actions. You register action handlers with `on_action`, and in tests you can dispatch actions programmatically through `VisualTestContext`. For the command launcher (Cmd+K), test the fuzzy matching and ranking logic separately from the UI that renders the results. I wrote about this in more detail in the [building a command launcher](/blog/building-command-launcher-gpui/) post.

The `AppRoute::parse_deep_link` tests show the pattern: parse input, assert output, assert errors on bad input. Your command handler tests should do the same.

## Why integration tests matter more for UI

Unit tests cover pure logic. But the bugs that hurt are the ones where logic meets rendering: a state transition that should trigger a re-render but does not, a subscription that fires for the wrong entity, a theme change that breaks layout.

GPUI's `VisualTestContext` lets you open a real window, render a component, dispatch actions, and read the resulting state. These tests are slower than plain `#[test]` because they go through the rendering pipeline. They are also the only tests that catch the "I forgot to call `cx.notify()`" class of bugs.

My rule of thumb: if a function signature includes `&mut Context<Self>`, it probably deserves an integration test. If it only takes `&self` or owned values, a unit test suffices.

## What the test suite looks like in practice

The project has around 20 tests across modules like `app_state`, `routes`, `undo_stack`, `events`, `storage`, `config_migrations`, and `testing`. Most are plain `#[test]` functions. A few use `tempfile::tempdir()` for filesystem operations. None require a GPU context because the architecture separates state from rendering.

The storage tests initialize a real SQLite database in a temp directory, run migrations, and verify the schema. This catches "did the migration actually create the table" bugs without any UI involved:

```rust
#[test]
fn initializes_schema_and_migration_table() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("app.db");
    let version = init_db(&db_path).expect("init db");
    assert_eq!(version, 1);

    let conn = rusqlite::Connection::open(&db_path).expect("open db");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 1",
            [],
            |row| row.get(0),
        )
        .expect("read migrations");
    assert_eq!(count, 1);
}
```

## The testing strategy in short

Test pure logic with plain `#[test]`. Test entity state and async behavior with `#[gpui::test]` and `TestAppContext`. Test rendering and action dispatch with `VisualTestContext` when you need to. Write hand-written fakes for external services. Do not test pixels.

For a deeper look at how gpui-starter structures state and context, see the [architecture guide](/docs/architecture/). If you are new to the framework, the [getting started with GPUI](/blog/getting-started-with-gpui/) post covers project setup and first components.
