---
title: "Building internal tools as Rust desktop apps"
description: "Why Rust desktop apps are a good fit for internal company tools: performance, security, single-binary distribution, and lower long-term maintenance costs."
date: 2026-06-05
tags: [Rust, internal-tools, desktop]
draft: false
---

Internal tools are the software that companies build for themselves. Inventory lookups, employee onboarding dashboards, deployment approval pipelines, compliance report generators. They are not flashy. They are not public-facing. But they keep the business running, and there are usually dozens of them inside any mid-size company.

Most internal tools end up as web apps. That makes sense: everyone has a browser, deployment is a single server, and you can hire React developers easily. But after maintaining internal tools for a few years, I keep hitting the same problems. Offline requirements that fight the browser. Security policies that require VPN plus SSO plus device attestation. Deployment pipelines that feel excessive for a tool that five people use.

Rust desktop apps solve several of these problems at once. Not all of them, and not without tradeoffs. But for the right kind of internal tool, a native binary is simpler to build, distribute, and maintain than a web app with all its supporting infrastructure.

## The case for desktop internal tools

The strongest argument for a desktop internal tool is distribution simplicity. A single binary file, copied to a machine, runs. No server to maintain. No database to patch. No container orchestration. For a tool used by 20 people in one office, a self-updating binary beats a Kubernetes deployment every time.

Security is the second argument. Internal tools handle sensitive data: employee records, financial figures, customer PII. A desktop app can store credentials in the OS keyring, encrypt local data with platform APIs, and operate entirely offline. The attack surface is the local machine, not a publicly accessible web endpoint.

Performance is the third. Some internal tools process real data. A warehouse tool might need to parse a 2 GB CSV export from a legacy system. A financial tool might run Monte Carlo simulations. These workloads are painful in a browser and natural in a native application.

The counter-argument is always development speed. Web tools are faster to build because the ecosystem is larger. That is true for the first version. It becomes less true as the server infrastructure accumulates and security requirements push you toward VPN gateways and SAML integrations that take weeks to set up.

## Why Rust specifically

You could build desktop internal tools in C++, C#, or Go. Rust has specific properties that fit this class of software.

A Rust binary statically links everything it needs. No .NET runtime, no JVM, no Python interpreter. This matters because internal tool machines are often locked down by IT policies that prevent installing runtimes.

Internal tools sometimes run for days: a dashboard on a monitoring screen, a data collection tool overnight. GC pauses during a screen share with a VP erode trust. Rust has no GC pauses.

Building for a different target is a `rustup target add` and a `cargo build --target` away. Finance team on Windows, engineering on macOS: two binaries from the same source.

Adding a dependency is one line in `Cargo.toml`. Building for release is `cargo build --release`. No `node_modules` bloating to 500 MB, no virtualenv juggling.

## What kinds of internal tools make sense

Not every internal tool should be a desktop app. A ticket tracker, a wiki, a shared calendar: these are collaborative tools that need a central server and concurrent access. Desktop apps are the wrong shape for them.

Where desktop internal tools shine:

Data transformation tools. CSV cleaners, report generators, data migration utilities. Single-user, run-once tools that do not need a server.

Monitoring dashboards. Real-time displays of system health or build status. A desktop app maintains persistent connections to data sources and renders updates at 60fps without a browser's overhead.

Offline-first tools. Field tools for warehouses, factories, or remote sites where network connectivity is unreliable. A desktop app with a local SQLite database works whether the network is up or not.

Security-sensitive tools. Credential managers, audit log viewers, compliance checkers where data should never leave the machine.

## A practical example: an inventory lookup tool

Let me sketch a real internal tool. Your warehouse team needs to look up inventory levels, locations, and reorder status. The data lives in PostgreSQL behind the corporate firewall. Today they use a web app that requires VPN, loads slowly over spotty Wi-Fi, and times out during peak hours.

A desktop alternative:

```rust
struct InventoryApp {
    db: Connection,
    search_query: String,
    results: Vec<InventoryItem>,
    last_sync: Instant,
}

struct InventoryItem {
    sku: String,
    name: String,
    quantity: i32,
    location: String,
    reorder_threshold: i32,
}

impl InventoryApp {
    fn init(cx: &mut AppContext) {
        // Open local SQLite cache
        let db = Connection::open("inventory_cache.db").unwrap();
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS items (
                sku TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                quantity INTEGER,
                location TEXT,
                reorder_threshold INTEGER
            )"
        ).unwrap();

        // Sync from server on startup (if network available)
        cx.spawn(async move |this, cx| {
            if let Ok(server_data) = fetch_from_server().await {
                this.update(cx, |app, cx| {
                    app.sync_from_server(&server_data);
                    app.last_sync = Instant::now();
                    cx.notify();
                }).ok();
            }
        }).detach();
    }
}
```

The app opens a local SQLite database on disk. On startup, it tries to sync from the server. If the network is down, it falls back to cached data. The warehouse team gets sub-millisecond lookups because everything is local.

The search rendering:

```rust
impl Render for InventoryApp {
    fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .p_4()
                    .child(
                        TextInput::new("search", &self.search_query)
                            .placeholder("Search SKU or name...")
                            .on_input(cx.listener(|this, text: &String, cx| {
                                this.search_query = text.clone();
                                this.results = this.search_local();
                                cx.notify();
                            }))
                    )
            )
            .child(
                div()
                    .flex_1()
                    .overflow_y_scroll()
                    .children(self.results.iter().map(|item| {
                        div()
                            .px_4()
                            .py_2()
                            .border_b_1()
                            .border_color(Color::Border)
                            .child(format!(
                                "{} {} qty:{} loc:{}",
                                item.sku, item.name, item.quantity, item.location
                            ))
                    }))
            )
    }
}
```

This is a complete working tool. No server to deploy. No VPN required after the initial sync. No browser tab someone accidentally closes. The binary is roughly 8 MB and starts in under a second.

## Distribution and updates

The biggest practical concern with desktop tools is keeping them updated. In a web app, you deploy once and everyone gets the new version. With a desktop binary, you need an update mechanism.

This is solvable. A self-updating binary checks a URL on startup, downloads a new version if available, and replaces itself. The update payload can be signed with Ed25519 so the binary rejects tampered downloads. GPUI's async runtime handles the download without blocking the UI.

```rust
fn check_for_updates(cx: &mut AppContext) {
    cx.spawn(async move |_this, cx| {
        let latest = fetch_latest_version().await;
        if latest.version > CURRENT_VERSION {
            let binary = download_update(&latest.url).await;
            if verify_signature(&binary, &latest.signature) {
                apply_update(&binary);
            }
        }
    }).detach();
}
```

For internal tools, the update URL points to your own infrastructure: an S3 bucket, a file share, or a static server. You control the rollout. No app store review process.

## Security considerations

Internal tools often handle data that should not leak. A desktop app has natural advantages here:

No browser DevTools. Users cannot open the network tab and inspect API calls. This is not real security (the binary can still be reverse-engineered), but it raises the bar for casual data exposure.

OS keyring integration. Credentials go into the system keychain instead of browser localStorage or a cookie.

Local encryption. SQLite supports SQLCipher for encrypting the database file at rest. Even if someone copies the file off the machine, they cannot read it without the key.

No CORS, no XSS, no CSRF. These web-specific attack classes do not apply to desktop apps. You still validate inputs and handle errors, but the attack surface shrinks.

This does not make desktop apps automatically secure. A poorly written Rust app can still leak data through log files or crash dumps. But the attack surface is smaller and more auditable than a web application with a server, a database, an API layer, and a frontend.

## Tradeoffs to be honest about

I do not want to pretend this approach has no downsides.

Platform support. GPUI currently supports macOS and Linux (via Vulkan). Windows support is in progress but not production-ready. Tauri is the safer cross-platform choice if Windows is a hard requirement.

UI development speed. Building a polished UI in GPUI takes longer than throwing together a React component. There is no npm registry of pre-built widgets. You build tables, forms, and navigation yourself. For a simple tool this adds days to the schedule. For a complex tool with custom layouts, the time difference shrinks because you are not fighting CSS and browser quirks.

Team skills. If your team knows JavaScript and nothing else, Rust's learning curve is real. It takes weeks to become productive. This is the strongest argument against Rust for a team that needs to ship next week.

Debugging. Rust's compile errors are excellent, but runtime UI debugging is harder than in a browser. There is no equivalent of Chrome DevTools for inspecting layout. You rely on logging and custom debug views.

## When to choose this path

Build your internal tool as a Rust desktop app when:

- The tool runs on controlled machines (company laptops, warehouse terminals).
- Offline access matters.
- The tool handles sensitive data that should stay local.
- The user base is small (under a few hundred).
- You want to avoid maintaining server infrastructure.

Stick with a web app when the tool is collaborative and needs real-time multi-user editing, the user base is large or includes external contractors, your team has no Rust experience and a tight deadline, or the tool is simple enough that a CRUD web app covers it.

For teams already writing Rust, or teams frustrated by the maintenance overhead of internal web applications, a desktop binary is worth considering. The [getting started guide](/docs/getting-started/) walks through setting up a GPUI project in under five minutes. I also wrote about [Electron alternatives](/blog/electron-alternatives-2026/) if you want to see how the options compare.

## Getting started with gpui-starter

[gpui-starter](https://github.com/hmziqrs/gpui-boilerplate) is a boilerplate I built to skip the infrastructure work when starting a GPUI project. It ships with multi-page navigation, 21 themes, i18n, form validation, a command palette, macOS tray integration, OS keyring credential storage, SQLite persistence, auto-updating with Ed25519 signature verification, and crash reporting. Clone it and have a working desktop app with all of that wired up in minutes. Setup instructions at [getting started](/docs/getting-started/).
