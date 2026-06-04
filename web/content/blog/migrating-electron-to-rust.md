---
title: "Migrating from Electron to Rust: lessons learned"
description: "Practical lessons from migrating a desktop application from Electron and JavaScript to Rust with GPUI. What breaks, what improves, and what surprises you."
date: 2026-06-05
tags: [Electron, Rust, migration]
draft: false
---

I spent two years building and maintaining an Electron app. Then I rewrote it in Rust with GPUI. This post is about what I learned doing that: the parts that went smoothly, the parts that hurt, and the things I wish someone had told me before I started.

The app was a data management tool with a sidebar, forms, SQLite storage, theming, and auto-updates. Nothing exotic. About 15,000 lines of TypeScript across the main and renderer processes. The rewrite ended up around 8,000 lines of Rust.

## Why I migrated

The Electron app worked fine for months. Then user counts grew, machines got older, and complaints about memory usage and startup time became regular. Slack alongside my app plus a browser tab or two would push 8GB laptops into swap. Users do not blame Slack. They blame the unfamiliar app.

The breaking point was a cold-start benchmark I ran out of curiosity: 2.8 seconds on a 2020 MacBook Air. That is slow enough that users wonder if the app crashed. A native menu bar app I had built with Swift launched in under 200 milliseconds. The gap bothered me enough to investigate alternatives.

I looked at [Tauri and GPUI side by side](/blog/electron-vs-tauri-vs-gpui/) and decided on GPUI. Tauri would have been the easier migration since it still uses web technologies for the UI layer. But I wanted to eliminate the browser entirely, and I was willing to accept a steeper learning curve to get there.

## Lesson 1: the mental model shift is the hardest part

In Electron, the UI is a web page. You think in components, props, CSS, and DOM events. State management means choosing between Redux, Zustand, or React Context. The rendering is someone else's problem.

GPUI works differently. You describe UI as a tree of Rust elements. There is no CSS file. Styling is method chaining on elements. State lives in `Entity<T>` structs that the framework tracks for changes.

```rust
struct ContactForm {
    name: String,
    email: String,
    errors: Vec<String>,
}

impl Render for ContactForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child(
                div()
                    .child(
                        Input::new(cx.entity().downgrade(), "name-input")
                            .placeholder("Name")
                            .text(self.name.clone())
                    )
            )
            .child(
                div()
                    .child(
                        Input::new(cx.entity().downgrade(), "email-input")
                            .placeholder("Email")
                            .text(self.email.clone())
                    )
            )
            .child(
                Button::new("submit", "Save")
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.validate_and_save(cx);
                    }))
            )
    }
}
```

This took me about three weeks to get comfortable with. The first week was fighting the borrow checker every time I tried to update state. The second week was understanding GPUI's entity system and why you cannot just mutate a field from a callback. The third week, things clicked.

GPUI entities are not React state variables. They are closer to model objects in a traditional MVC architecture. You own the data, the framework owns the rendering, and `cx.notify()` is your signal that something changed.

## Lesson 2: some things get easier

I expected the rewrite to be harder across the board. It was not. Several things turned out simpler in Rust than in TypeScript.

**Serialization and persistence.** In Electron, I used a custom JSON-on-disk layer with hand-written migration scripts for schema changes. Moving to [SQLite with rusqlite](/blog/sqlite-rust-desktop-tutorial/) gave me proper transactions, typed columns, and schema migrations in about 200 lines of code. The SQL is embedded in Rust, compile-time checked for syntax errors via string patterns, and the connection handling is explicit rather than hidden behind an async ORM.

```rust
fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS contacts (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_contacts_email
            ON contacts(email);"
    )?;
    Ok(())
}
```

**Error handling.** TypeScript's exception model means any function can throw anything. You discover the failure at runtime, often in production. Rust forces you to acknowledge every error path with `Result<T, E>`. This felt tedious for the first week. By week two, I realized I was catching bugs that would have shipped in the Electron version. The compiler made me handle the database-locked case. It made me handle the malformed-UTF8 case. These were bugs I had fixed in production before.

**Build and deployment.** Electron needs electron-builder, code signing setup, auto-update feed hosting, and a CI pipeline that downloads a 200MB Chromium binary before it can start. The GPUI build produces a single binary. Code signing is one `codesign` call. Auto-updates are a GitHub Release with Ed25519-signed manifests. The CI pipeline went from 12 minutes to 4 minutes because there is no Chromium to download.

## Lesson 3: some things get harder

The rewrite was not all improvements.

**The component ecosystem does not exist.** In Electron, I could pull date pickers, rich text editors, charting libraries, and drag-and-drop utilities from npm. In GPUI, I built most of these from scratch. A date picker took a day. A form validation system took two days. A sidebar with keyboard navigation took another day. None of these are complicated in isolation, but they add up.

I ended up building reusable versions of these components, which is how [gpui-form](/blog/form-validation-rust-tutorial/) and the sidebar component in [gpui-starter](https://github.com/hmziqrs/gpui-boilerplate) came to exist.

**Compile times are real.** The Electron app had hot reload. Change a CSS value, see it instantly. Change a React component, see it in under a second. GPUI compiles Rust. Incremental builds on my M2 MacBook take 3 to 6 seconds. Full builds after a dependency change take 30 to 60 seconds. You learn to batch your changes and think more carefully before saving, which is not always a bad thing, but it does slow down the experimentation loop.

**Debugging UI is harder without DevTools.** In Electron, you open Chrome DevTools and inspect the DOM, check computed styles, profile rendering, and watch state updates in real time. GPUI has none of that. When a layout is wrong, you reason about it from the code. When a render is slow, you add timing instrumentation. I wrote a frame-time debugger that logs render durations, which helped, but it is not the same as a visual DOM inspector.

## Lesson 4: architecture translates more than you think

One pleasant surprise: the high-level architecture of the Electron app mapped onto the Rust version with relatively little distortion.

The sidebar + main content layout became the same structure, just built with GPUI's flex primitives instead of CSS grid. The route-based navigation became a match on the current page enum instead of a React Router config. The settings store became a Rust struct with [Fluent-based i18n](/blog/integrating-fluent-rust-i18n/) instead of i18next JSON files.

```rust
enum Route {
    Dashboard,
    Contacts,
    Settings,
}

impl Render for App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .h_full()
            .child(self.render_sidebar(window, cx))
            .child(
                div()
                    .flex_grow()
                    .child(match &self.route {
                        Route::Dashboard => self.render_dashboard(window, cx),
                        Route::Contacts => self.render_contacts(window, cx),
                        Route::Settings => self.render_settings(window, cx),
                    })
            )
    }
}
```

If your Electron app has a clean separation between state logic and rendering, the migration will be smoother. If your business logic is tangled into React hooks and useEffect callbacks, you will need to detangle it first. That detangling is worth doing regardless of whether you migrate.

## Lesson 5: performance is not the only reason

I started the migration to fix memory usage and startup time. I got both: RAM dropped from ~400MB to ~60MB, and cold start dropped from 2.8 seconds to ~350ms. Those numbers are real and measurable.

But the thing I did not expect was how much more predictable the app became. No more GC pauses during heavy data processing. No more 'works on my machine' rendering differences between Chromium versions. No more npm audit producing 47 vulnerabilities on a Tuesday morning.

The tradeoff is hiring. Finding Rust developers is harder than finding JavaScript developers. The team that maintains this app needs to be comfortable with systems programming. For a small team or solo project, that might not matter. For a growing company, it is a real constraint.

## What I would do differently

If I were starting this migration again, I would:

1. Port the data layer first, not the UI. Get SQLite running, migrate the schema, write tests against the persistence layer. This is the highest-value part to get right early.

2. Build the components I need before wiring up screens. I tried to build screens and components simultaneously, which led to a lot of rework once the component APIs stabilized.

3. Budget more time than you think. I estimated 6 weeks. It took 10. The extra 4 weeks were split between learning GPUI patterns and building components that npm would have given me for free.

4. Keep the Electron version running in parallel during the migration. Being able to reference the old app for behavior questions saved hours of guessing.

## Getting started

If you are considering a similar migration, the [gpui-starter](https://github.com/hmziqrs/gpui-boilerplate) project includes a working GPUI app with sidebar navigation, form validation, SQLite persistence, theming, i18n, auto-updates, and the other pieces you would otherwise build from scratch. The [getting started guide](/docs/getting-started/) walks through setup and project structure. It will not make the mental model shift easier, but it will save you from reinventing the infrastructure pieces.
