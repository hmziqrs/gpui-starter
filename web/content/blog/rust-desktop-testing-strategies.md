---
title: "Testing strategies for Rust desktop applications"
description: "How to test Rust desktop apps: unit tests, integration tests, UI tests, and snapshot testing strategies that work."
date: 2026-06-05
tags: [Rust, testing, strategies]
draft: false
---

I shipped a Rust desktop app to users for six months before writing my first test. Every release introduced a regression I hadn't caught. A config migration that failed silently on empty files. A sidebar that forgot its collapsed state after a theme change. An undo stack that panicked when you undid past the first action.

These were not hard bugs. They were the kind you catch in five minutes with a test suite. The problem was that I didn't have one, and I didn't know how to build one for a desktop app. The testing advice I found was either for web apps or for Rust libraries. Neither quite fit.

This post covers the testing strategies that work for Rust desktop applications: unit tests that run in milliseconds, integration tests that catch wiring bugs, and snapshot tests that catch regressions.

## Start with unit tests for business logic

Most of the code in a desktop app is logic, not UI. Parsing, validation, state transitions, serialization, routing, config migration. These are pure functions or methods on plain structs. You test them the same way you test any Rust code: `#[test]` functions with `assert!` and `assert_eq!`.

The trick is making sure your architecture supports this. If your state is tangled inside your render function, you can't test it without spinning up a GPU context. Keep state in standalone modules.

```rust
#[test]
fn config_migration_v0_to_v2_applies_both_migrations() {
    let raw_v0 = json!({
        "version": 0,
        "theme": "dark",
        "sidebar_collapsed": true
    });

    let result = migrate_config(raw_v0).expect("migration should succeed");

    assert_eq!(result["version"], 2);
    assert_eq!(result["theme"], "dark");
    assert_eq!(result["sidebar_collapsed"], true);
    assert_eq!(result["notifications_enabled"], true); // added in v2
}
```

This test runs in under a millisecond. No window, no event loop, no GPU. Just a JSON value and a function call.

Cover every config migration, every parser, every state machine transition. These are the bugs that bite users and cost nothing to test. For more on how this works in a real project, see the [testing guide](/docs/testing/).

## Integration tests catch wiring mistakes

Unit tests tell you that each piece works in isolation. Integration tests tell you that the pieces work together. In a desktop app, the seams where things break are the boundaries between modules: state and persistence, commands and actions, config and UI.

The Rust convention is to put integration tests in `tests/` as separate binary crates. For desktop apps, I prefer `#[cfg(test)]` modules with shared utilities. Either way works.

The scenarios worth covering at the integration level:

- Database initialization, migration, and read/write cycles
- Config file loading, migration, and saving
- Command routing: input event produces the right state change
- Credential storage round-trips through the OS keyring

```rust
#[test]
fn sqlite_store_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");

    let store = Store::open(&db_path).expect("open store");
    store.put("key1", b"value1").expect("put");
    store.put("key2", b"value2").expect("put");

    let store = Store::open(&db_path).expect("reopen store");
    assert_eq!(store.get("key1").unwrap(), Some(b"value1".to_vec()));
    assert_eq!(store.get("key2").unwrap(), Some(b"value2".to_vec()));
    assert_eq!(store.get("missing").unwrap(), None);
}
```

This test uses a real SQLite database in a temp directory. No mocks, no stubs. The database actually writes to disk. This catches bugs like "the migration didn't create the expected index" that a mock would miss.

If you're working with SQLite in your desktop app, the [SQLite integration tutorial](/blog/sqlite-rust-desktop-tutorial/) goes deeper into schema design and test patterns.

## Test error paths on purpose

Happy path testing is easy. But users don't feed your app good input. They delete config files while the app is running. They disconnect the network mid-request. They paste 4MB of text into a search field.

Rust makes error handling explicit with `Result`, which makes error path testing explicit too. Test every `Err` branch. If a function can fail, write a test that makes it fail.

```rust
#[test]
fn store_handles_corrupted_database() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("corrupt.db");
    std::fs::write(&db_path, b"not a sqlite file").expect("write junk");

    let result = Store::open(&db_path);
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("database") || msg.contains("format"),
        "error message should mention the problem, got: {}", msg);
}
```

Notice the assertion on the error message. Users see these messages. Making sure they say something useful is a test worth having. Read more about error handling in [error boundaries for Rust GUI apps](/blog/rust-desktop-error-boundaries/).

## Fake services, not mocks

Desktop apps talk to external systems: the filesystem, the OS keyring, notification APIs, network services. You need test doubles. The two options are mocks (verify method calls) and fakes (working implementations with simplified behavior).

I prefer fakes. A mock tells you a method was called. A fake tells you what happened as a result.

```rust
pub struct FakeKeyring {
    store: HashMap<String, String>,
    pub should_fail: bool,
}

impl FakeKeyring {
    pub fn set(&mut self, service: &str, key: &str, value: &str) -> Result<()> {
        if self.should_fail { return Err(anyhow!("unavailable")); }
        self.store.insert(format!("{}:{}", service, key), value.to_string());
        Ok(())
    }

    pub fn get(&self, service: &str, key: &str) -> Result<String> {
        if self.should_fail { return Err(anyhow!("unavailable")); }
        self.store.get(&format!("{}:{}", service, key))
            .cloned().ok_or_else(|| anyhow!("not found"))
    }
}
```

This fake stores credentials in memory, round-trips `set` and `get` correctly, and has a `should_fail` flag for error paths. More code than a mock, but it catches bugs that mocks miss.

## Snapshot testing for structured data

Snapshot testing captures the output of a function and compares it to a stored reference. When the output changes, the test fails and you see the diff. Accept the change or fix the bug.

This works well for things that are annoying to assert manually: large JSON configs, generated SQL schemas, serialized state, theme definitions. Instead of writing twenty `assert_eq!` lines, you snapshot the whole thing.

```rust
#[test]
fn theme_snapshot() {
    let theme = Theme::catppuccin_mocha();
    insta::assert_json_snapshot!(theme);
}
```

The [insta](https://insta.rs) crate is the standard choice. On first run it creates a snapshot file. On subsequent runs it compares against that file. If something changes, you get a color diff and can review with `cargo insta review`.

Snapshot tests are cheap to write and catch unintended changes. The downside is stale snapshots that accrete when you accept changes without reading the diff. Treat snapshot review like code review.

## UI testing: test behavior, not pixels

Testing the visual output of a desktop app is harder than testing a web app. There is no DOM to query. The UI is rendered by the GPU through a framework-specific pipeline. Pixel-level assertions are fragile.

The practical approach: test UI behavior through the same APIs the app uses internally. Dispatch actions, update entities, read state back.

```rust
#[gpui::test]
fn theme_change_updates_global(cx: &mut TestAppContext) {
    let theme_state = cx.new(|_| ThemeState::default());

    theme_state.update(cx, |state, cx| {
        state.set_theme("catppuccin_mocha", cx);
    });

    let name = theme_state.read_with(cx, |state, _| state.current_name());
    assert_eq!(name, "catppuccin_mocha");
}
```

This uses a GPUI test context that runs without a window. It tests the same code path the UI triggers, minus the rendering step. Fast and deterministic.

For visual regression testing across theme changes, the [theme system tutorial](/blog/theme-system-gpui-tutorial/) covers a pattern for rendering components in every theme and comparing output.

## Test organization for desktop apps

I structure tests in three buckets, each with a clear purpose and speed profile:

**Unit tests** (`#[test]` in `src/`). Pure logic. No IO, no async runtime, no GPU. Target: under 1ms per test. Run these on every commit.

**Integration tests** (`tests/` or `#[cfg(test)]` modules with shared setup). Cross-module behavior with real IO. Temp directories, real databases, actual file writes. Target: under 100ms per test. Run these before push.

**UI tests** (`#[gpui::test]` or equivalent). State changes through the framework context. No pixel assertions. Target: under 500ms per test. Run these in CI.

Keep the unit test bucket large. Every test you write at the unit level is a test you don't have to maintain at the integration level.

## CI configuration

Desktop app CI needs to handle platform-specific dependencies. On macOS, GPU tests need a window server. On Linux, you need a virtual framebuffer (`Xvfb`). macOS GitHub Actions runners have a window server by default. On Linux, start `Xvfb` before your test step:

```yaml
- name: Start virtual framebuffer
  if: runner.os == 'Linux'
  run: Xvfb :99 -screen 0 1024x768x24 &
- name: Run tests
  run: cargo test --features test-support
  env:
    DISPLAY: ":99"
```

Skip GPU tests on CI if your framework doesn't support headless rendering.

## What to skip

Not everything needs a test. Skip:

- Third-party library behavior (test your wrapper, not their code)
- Framework-generated code (GPUI elements, Tauri commands)
- Visual styling (colors, spacing, font sizes)
- One-line getters and setters

Focus test effort on code that encodes decisions: business rules, data transformations, state transitions, error handling. If a bug in this code would confuse a user or lose data, write a test.

For a working example of all these strategies in a real desktop app, [gpui-starter](https://github.com/freeoxide/gpui-starter) includes unit tests, integration tests, fake services, and snapshot tests across its modules. The [getting started guide](/docs/getting-started/) walks through the project structure and test setup.
