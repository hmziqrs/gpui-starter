---
title: "Testing GPUI Applications"
description: "Strategies for testing Rust desktop apps built with GPUI: unit tests, entity tests, snapshot tests, and integration harnesses using TestAppContext"
---

Testing in gpui-starter splits into two tracks: plain `#[test]` for logic that runs without GPUI, and `#[gpui::test]` when you need the app context, entity system, or async executor. The [architecture](/docs/architecture) page explains how these layers connect. This page covers the specifics of writing and organizing tests for each layer.

If you are new to the project, start with [getting started](/docs/getting-started) to get the build running, then come back here.

## Choosing the right test attribute

GPUI provides two test attributes. Pick based on what your test touches:

| Attribute | When to use | What you get |
|-----------|------------|--------------|
| `#[test]` | Pure logic, parsing, validation, data transforms | Nothing extra. Standard Rust test. |
| `#[gpui::test]` | Entity creation (`cx.new()`), globals, subscriptions | `&mut TestAppContext` with a deterministic executor |
| `#[gpui::test] async` | Background tasks, timers, channels | `&mut TestAppContext` that can `run_until_parked()` |
| `#[gpui::test(iterations = 10)]` | Randomized property tests | `&mut TestAppContext` + `mut StdRng` |

The rule is simple: if your test calls `cx.new()`, `cx.spawn()`, reads a global, or opens a window, it needs `#[gpui::test]`. Everything else is plain `#[test]`.

## Unit tests: logic without GPUI

Pure data models, validation rules, and state machines do not need a GPUI context. These are regular Rust tests. They compile fast and run in milliseconds.

The undo system in gpui-starter uses an internal `UndoModel` struct that tracks past and future stacks. Testing it is a plain `#[test]` because the model is a plain struct with no GPUI dependency:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> UndoEntry {
        UndoEntry {
            label: "Switch Theme".to_string(),
            undo_label: "Undo Theme Switch".to_string(),
            redo_label: "Redo Theme Switch".to_string(),
            created_at: AppTimestamp::now(),
            kind: UndoKind::ThemeMode {
                before: ThemeMode::Light,
                after: ThemeMode::Dark,
            },
        }
    }

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

    #[test]
    fn pop_undo_sets_rejected_reason_when_empty() {
        let mut model = UndoModel::default();
        assert!(model.pop_undo().is_none());
        assert_eq!(model.last_rejected.as_deref(), Some("nothing to undo"));
    }
}
```

When your test touches the filesystem, use `tempfile::tempdir()` so each test gets an isolated directory. The storage initialization tests in `tests/e2e_lifecycle.rs` do this for SQLite:

```rust
#[test]
fn test_storage_initializes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("app.db");

    let conn = rusqlite::Connection::open(&db_path).expect("open connection");
    let version = gpui_starter::db_migrations::run_migrations(&conn).expect("run migrations");
    assert_eq!(version, 2, "migrations should bring schema to version 2");
}
```

The temp directory cleans up automatically when the `TempDir` value drops. No manual cleanup needed.

## Testing with TestAppContext

`TestAppContext` gives you a single-threaded, deterministic executor. No threads, no race conditions. Entity creation, updates, and reads all work the same as production code.

### Entity state

Create an entity, update it, and read it back:

```rust
#[gpui::test]
fn test_entity_state(cx: &mut TestAppContext) {
    let entity = cx.new(|cx| Counter::new(cx));

    let initial = entity.read_with(cx, |counter, _| counter.count);
    assert_eq!(initial, 0);

    entity.update(cx, |counter, cx| {
        counter.count = 42;
        cx.notify();
    });

    let updated = entity.read_with(cx, |counter, _| counter.count);
    assert_eq!(updated, 42);
}
```

The [architecture](/docs/architecture) page has more detail on how entities work in GPUI if you need background.

### Window-dependent tests

Tests that render views or dispatch actions need a window. Open one and convert to `VisualTestContext`:

```rust
use gpui::VisualTestContext;

#[gpui::test]
fn test_with_window(cx: &mut TestAppContext) {
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| MyView::new(cx))
        }).unwrap()
    });

    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let root = window.root(&mut cx).unwrap();
    // interact with root entity...
}
```

The `tests/support/rendering.rs` module wraps this pattern into reusable helpers. `open_visual_context(cx)` creates a minimal window, and `open_window_with_root(cx, |window, cx| MyView::new(window, cx))` opens one with your own root entity.

## Async tests

Async GPUI tests use `#[gpui::test]` on an `async fn`. The main method to know is `cx.run_until_parked()`, which flushes all pending tasks and timers until nothing is left to run.

```rust
#[gpui::test]
async fn test_async_task(cx: &mut TestAppContext) {
    let entity = cx.new(|cx| MyComponent::new(cx));

    entity.update(cx, |comp, cx| comp.start_background_update(cx));

    // Detached tasks have not run yet.
    let before = entity.read_with(cx, |comp, _| comp.value);
    assert_eq!(before, 0);

    cx.run_until_parked();

    let after = entity.read_with(cx, |comp, _| comp.value);
    assert_eq!(after, 10);
}
```

A common mistake is forgetting `run_until_parked()` and then asserting on state that has not been computed yet. If your test fails on a value that should have been set by a spawned task, add `cx.run_until_parked()` before the assertion.

For tests involving real threads or OS sockets (not just GPUI tasks), call `cx.executor().allow_parking()` so the executor can block on external events without deadlocking.

## Mocking external services

The `src/testing.rs` module ships fake implementations for every external dependency. Each fake is a plain struct with no GPUI dependency, so they work in both `#[test]` and `#[gpui::test]` contexts.

| Fake | Key fields | What it does |
|------|-----------|--------------|
| `FakeTelemetrySink` | `events: VecDeque<String>`, `flushed: bool` | Records events and errors in memory |
| `FakeConnectivityProbe` | `next_ok: bool` | Returns `Ok` or `Err` based on the flag |
| `FakeNotificationBackend` | `sent: VecDeque<String>`, `fail_send: bool` | Records sent notifications; toggle errors |
| `FakeSecureStorage` | (internal `Option<String>`) | In-memory secret storage with set/get/delete |

Build a fake, configure it, and pass it to the code under test. The fakes have their own tests in `src/testing.test.rs`:

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

For GPUI globals, call `cx.set_global()` to install a fake and `cx.remove_global()` to clean up. Globals are scoped to the test context, not permanent. Replace them freely between test cases.

## Testing form validation

Form structs derive `Koruma` via the `gpui-form` + `koruma` integration. Each field gets a `validate()` method generated at compile time. You can test validation on plain struct instances without GPUI. See the [forms](/docs/forms) docs for the full setup.

```rust
#[test]
fn empty_name_fails_validation() {
    let mut form = RegistrationForm::default();
    form.name = String::new();
    let result = form.validate();
    assert!(result.is_err());
}

#[test]
fn valid_form_passes() {
    let mut form = RegistrationForm::default();
    form.name = "Ada Lovelace".to_string();
    form.email = "ada@example.com".to_string();
    form.password = "secret".to_string();
    form.phone = "(555) 123-4567".to_string();
    form.website = "https://example.com".to_string();
    assert!(form.validate().is_ok());
}
```

Each validator (`NonEmptyValidation`, `EmailValidation`, `PhoneNumberValidation`, `UrlValidation`) is tested independently. The [form validation with Koruma](/blog/form-validation-koruma-rust) blog post walks through adding custom validators.

## Testing actions and commands

Actions are dispatched through a window's focus handle. This requires a `VisualTestContext`:

```rust
actions!(my_app, [Increment]);

#[gpui::test]
fn test_action_dispatch(cx: &mut TestAppContext) {
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| Counter::new(cx))
        }).unwrap()
    });

    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let counter = window.root(&mut cx).unwrap();

    let handle = counter.read_with(&cx, |c, _| c.focus_handle.clone());
    cx.update(|window, cx| {
        handle.dispatch_action(&Increment, window, cx);
    });

    let count = counter.read_with(&cx, |c, _| c.count);
    assert_eq!(count, 1);
}
```

The [command launcher](/docs/command-launcher) docs cover how actions get bound to keyboard shortcuts in the real app. For tests, you dispatch them directly.

## Testing event subscriptions

Entities that implement `EventEmitter<T>` can emit typed events. Subscribe during construction and verify the handler receives them:

```rust
#[derive(Clone)]
struct ValueChanged { new_value: i32 }

impl EventEmitter<ValueChanged> for MyComponent {}

#[gpui::test]
fn test_event_emission(cx: &mut TestAppContext) {
    let component = cx.new(|cx| {
        cx.subscribe_self(|this, event: &ValueChanged, cx| {
            this.received_value = event.new_value;
            cx.notify();
        });
        MyComponent::default()
    });

    component.update(cx, |_, cx| {
        cx.emit(ValueChanged { new_value: 123 });
    });

    let received = component.read_with(cx, |comp, _| comp.received_value);
    assert_eq!(received, 123);
}
```

## Snapshot tests

gpui-starter uses `insta` for snapshot testing serializable structures. Snapshot tests are useful when you want to lock down the shape of serialized output (JSON, YAML) and catch regressions when the structure changes.

The `tests/snapshot_tests.rs` file covers config serialization, route parsing, theme file structure, and update manifests:

```rust
#[test]
fn test_config_default_serialization() {
    let config = AppConfig::default();
    insta::assert_yaml_snapshot!("config_default", &config);
}

#[test]
fn test_config_roundtrip() {
    let original = AppConfig {
        version: 1,
        theme: "Gruvbox Dark".to_string(),
        locale: "en".to_string(),
        // ... other fields
    };
    let json = serde_json::to_string(&original).expect("serialize config");
    let restored: AppConfig = serde_json::from_str(&json).expect("deserialize config");
    assert_eq!(original, restored);
    insta::assert_yaml_snapshot!("config_roundtrip", &serde_json::to_value(&original).unwrap());
}
```

Run `cargo insta review` after adding or modifying snapshot tests to accept or reject changes. Snapshots live in `tests/snapshots/` as `.snap` files checked into git.

## Integration tests

Integration tests live in the `tests/` directory at the crate root. They exercise cross-module behavior that unit tests cannot reach.

### Documentation and QA checks

The `tests/qa_docs.rs` file verifies that documentation files exist and contain expected sections. This catches silent regressions in docs:

```rust
#[test]
fn qa_matrix_contains_core_cases() {
    let content = std::fs::read_to_string("docs/qa-matrix.md").expect("read qa matrix");
    let normalized = content.to_lowercase();
    assert!(normalized.contains("second-instance forwarding"));
    assert!(normalized.contains("open logs folder"));
    assert!(normalized.contains("secure storage unavailable path"));
}
```

### Navigation and routing

The `tests/e2e_navigation.rs` file tests deep-link parsing and sidebar page registration without launching GPUI:

```rust
#[test]
fn test_route_parsing() {
    let cases = &[
        ("gpui-starter://home", AppRoute::Page(Page::Home)),
        ("gpui-starter://form", AppRoute::Page(Page::Form)),
        ("gpui-starter://settings", AppRoute::Page(Page::Settings)),
    ];

    for (url, expected) in cases {
        let parsed = AppRoute::parse_deep_link(url)
            .unwrap_or_else(|e| panic!("failed to parse {url}: {e}"));
        assert_eq!(parsed, *expected);
    }
}
```

The [routing](/docs/routing) page explains the deep-link scheme and route matching in detail.

### Lifecycle tests

The `tests/e2e_lifecycle.rs` file tests app initialization without the GPUI event loop: panic hooks, crash markers, config loading, and database migration.

## Property testing

Use `#[gpui::test(iterations = N)]` with a `mut rng: StdRng` parameter to run randomized tests. Each iteration gets a fresh seed:

```rust
use rand::rngs::StdRng;

#[gpui::test(iterations = 10)]
fn test_counter_random_operations(cx: &mut TestAppContext, mut rng: StdRng) {
    let counter = cx.new(|cx| Counter::new(cx));
    let mut expected = 0i32;

    for _ in 0..100 {
        let delta = rng.random_range(-10..=10);
        expected += delta;
        counter.update(cx, |c, cx| { c.count += delta; cx.notify(); });
    }

    let actual = counter.read_with(cx, |c, _| c.count);
    assert_eq!(actual, expected);
}
```

For a broader look at how testing fits into the development workflow, see [testing strategies for Rust desktop apps](/blog/rust-desktop-testing-strategies).

## Running tests

```bash
# Run all tests
cargo test

# Run tests in a specific module
cargo test routes::tests

# Run a single test by name
cargo test pop_undo_sets_rejected_reason

# Show println output (hidden by default)
cargo test -- --nocapture

# Run with backtrace on failure
RUST_BACKTRACE=1 cargo test

# Run only snapshot tests and review changes
cargo test --test snapshot_tests
cargo insta review

# Run the integration suite
cargo test --test e2e_lifecycle --test e2e_navigation --test qa_docs
```

## Test organization

Group related tests into `mod tests` within each source file. Use helper functions for repeated setup. The pattern in gpui-starter is `#[cfg(test)] #[path = "module_name.test.rs"] mod module_name_test;` to keep test code in a separate file:

```rust
// src/services/undo_stack.rs
#[cfg(test)]
#[path = "undo_stack.test.rs"]
mod undo_stack_test;
```

```rust
// src/services/undo_stack.test.rs
use super::*;

fn sample_entry() -> UndoEntry {
    UndoEntry {
        label: "Switch Theme".to_string(),
        undo_label: "Undo Theme Switch".to_string(),
        redo_label: "Redo Theme Switch".to_string(),
        created_at: AppTimestamp::now(),
        kind: UndoKind::ThemeMode {
            before: ThemeMode::Light,
            after: ThemeMode::Dark,
        },
    }
}

#[test]
fn record_clears_redo_history() {
    let mut model = UndoModel {
        future: vec![sample_entry()],
        ..UndoModel::default()
    };
    model.record(sample_entry());
    assert!(model.future.is_empty());
}
```

This keeps production source files clean while putting tests right next to the code they exercise.
