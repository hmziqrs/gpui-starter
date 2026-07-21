---
title: "HTTP requests in Rust desktop apps with reqwest"
description: "How to use reqwest for HTTP requests in Rust desktop applications: async patterns, error handling, and response parsing with real code examples."
date: 2026-06-05
tags: [Rust, HTTP, reqwest]
draft: false
---

Desktop applications need to talk to APIs. Whether you're fetching user data, downloading files, or calling a backend service, you need an HTTP client that works well with Rust's async runtime. reqwest is the standard choice: it handles TLS, cookies, redirects, and JSON out of the box.

This post covers how I use reqwest in Rust desktop apps, including async patterns that work with GPUI's entity system, practical error handling, and response parsing.

## Why reqwest

Rust has several HTTP client crates. [hyper](https://hyper.rs/) is the low-level foundation. [ureq](https://github.com/algesten/ureq) is a simpler synchronous option. reqwest sits on top of hyper and gives you an ergonomic async API with reasonable defaults.

The main reasons I reach for reqwest:

- It defaults to HTTPS with native TLS. No extra configuration for most cases.
- It supports streaming request and response bodies, which matters for large downloads or uploads.
- JSON deserialization with serde is built in. One method call to go from an HTTP response to a typed struct.
- Connection pooling and cookie handling are opt-in with a couple of lines.

If you are already using [tokio](https://tokio.rs/) or another async runtime, reqwest fits in without friction. If you are building a GPUI app, the story is slightly different because GPUI has its own async dispatch, but the integration is straightforward.

## Basic setup

Add reqwest to your `Cargo.toml`:

```toml
[dependencies]
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
```

The `json` feature pulls in serde and gives you the `json()` method on responses. You almost always want this.

A simple GET request looks like this:

```rust
use reqwest::Error;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let body: String = reqwest::get("https://api.example.com/status")
        .await?
        .text()
        .await?;

    println!("Response: {body}");
    Ok(())
}
```

That handles DNS resolution, TLS negotiation, the HTTP request, and reading the full response body in four lines. The `?` operator propagates errors up, which is the idiomatic Rust approach.

## Typed responses with serde

Raw strings are fine for debugging, but real apps need typed data. Define a struct, derive `Deserialize`, and reqwest handles the rest.

```rust
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct User {
    id: u64,
    name: String,
    email: String,
}

async fn fetch_user(id: u64) -> Result<User, reqwest::Error> {
    let url = format!("https://api.example.com/users/{id}");
    let user: User = reqwest::get(&url)
        .await?
        .json()
        .await?;

    Ok(user)
}
```

If the API returns a field that doesn't match your struct, serde will give you a clear error message telling you which field failed and why. This is one of those areas where Rust's type system really pays off. You catch schema mismatches early instead of debugging `undefined` in production.

For nested responses, nest your structs:

```rust
#[derive(Deserialize, Debug)]
struct ApiResponse {
    data: Vec<User>,
    total: u64,
    page: u32,
}
```

## Error handling that doesn't hide failures

reqwest's `Error` type covers connection failures, HTTP status errors (if you opt in), and body parsing errors. By default, reqwest does not treat a 404 or 500 as an error. The request succeeds, and you get the response body. You need to opt into status code checking with `error_for_status()`.

Here is the pattern I use in production:

```rust
use reqwest::StatusCode;

enum AppError {
    Network(reqwest::Error),
    Api { status: u16, message: String },
    Parse(reqwest::Error),
}

async fn fetch_with_error_handling(
    client: &reqwest::Client,
    url: &str,
) -> Result<User, AppError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(AppError::Network)?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Api { status, message: body });
    }

    response.json::<User>().await.map_err(AppError::Parse)
}
```

This separates network failures (DNS, timeout, connection refused) from API-level errors (404, 403, 500) from parse errors (unexpected JSON shape). Each variant can be handled differently in your UI. A network error might trigger a retry, while a 403 means you need to re-authenticate.

## Reusing a client instance

Creating a new client for every request wastes resources. A `reqwest::Client` maintains a connection pool internally. Create it once and share it across your app.

```rust
use std::sync::Arc;

struct AppState {
    http_client: reqwest::Client,
    base_url: String,
}

impl AppState {
    fn new(base_url: String) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("failed to build HTTP client");

        Self { http_client, base_url }
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, AppError> {
        let url = format!("{}{path}", self.base_url);
        let response = self.http_client
            .get(&url)
            .send()
            .await
            .map_err(AppError::Network)?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Api { status, message: body });
        }

        response.json::<T>().await.map_err(AppError::Parse)
    }
}
```

Wrapping the client in a struct lets you set consistent timeouts, default headers, and base URLs in one place. Every call through `AppState::get` gets the same configuration.

## Adding authentication headers

Most APIs require authentication. reqwest handles this through request builders:

```rust
async fn fetch_authenticated(
    client: &reqwest::Client,
    token: &str,
    url: &str,
) -> Result<User, reqwest::Error> {
    client
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await?
        .json()
        .await
}
```

For apps that make many authenticated requests, consider adding a default header to the client builder:

```rust
let client = reqwest::Client::builder()
    .default_headers({
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {token}")
                .parse()
                .expect("invalid token"),
        );
        headers
    })
    .build()?;
```

This is where [secure credential storage](/docs/secure-storage/) becomes relevant. You don't want to hardcode tokens or store them in plain text files. On macOS, use the system keychain. If you are building a GPUI app, check out the [secure storage guide](/blog/secure-storage-rust-tutorial/) for how to integrate keyring-rs properly.

## POST requests and JSON bodies

Sending data to an API is just as straightforward. Serialize a struct with serde and pass it to the `json()` method on the request builder:

```rust
#[derive(serde::Serialize)]
struct CreatePost {
    title: String,
    body: String,
    published: bool,
}

async fn create_post(
    client: &reqwest::Client,
    post: &CreatePost,
) -> Result<Post, reqwest::Error> {
    client
        .post("https://api.example.com/posts")
        .json(post)
        .send()
        .await?
        .json()
        .await
}
```

reqwest sets the `Content-Type: application/json` header automatically when you use `.json()`. One less thing to forget.

## Using reqwest with GPUI

GPUI has its own async dispatch system built around entities. You don't call `#[tokio::main]` directly. Instead, you spawn async tasks through the GPUI context.

Here is what that looks like in practice:

```rust
use gpui::{App, Entity, AsyncWindowContext};
use serde::Deserialize;

#[derive(Deserialize)]
struct ReleaseInfo {
    version: String,
    notes: String,
}

struct UpdateService {
    client: reqwest::Client,
    latest: Option<ReleaseInfo>,
}

impl UpdateService {
    fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("failed to build client");

        Self {
            client,
            latest: None,
        }
    }

    fn check_for_updates(&mut self, cx: &mut AsyncWindowContext) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result: Result<ReleaseInfo, reqwest::Error> = client
                .get("https://api.example.com/releases/latest")
                .send()
                .await?
                .json()
                .await;

            match result {
                Ok(info) => {
                    this.update(cx, |service, _cx| {
                        service.latest = Some(info);
                    })?;
                }
                Err(e) => {
                    eprintln!("Update check failed: {e}");
                }
            }

            Ok(())
        })
        .detach();
    }
}
```

The important part is `cx.spawn()`. It takes an async block and runs it on GPUI's runtime. The `this` handle lets you update the entity's state from inside the async block, and GPUI handles the thread safety. No mutex, no channels, no manual synchronization.

This pattern works for any kind of background HTTP work: checking for updates, fetching data for a view, uploading logs. For more on GPUI's entity system, see the [state management guide](/blog/state-management-gpui-entities/).

## Timeouts and retries

Network requests fail. What matters is how your app recovers.

Always set timeouts. A request that hangs forever will stall whatever part of your UI is waiting on it:

```rust
let client = reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(10))
    .connect_timeout(std::time::Duration::from_secs(5))
    .build()?;
```

For retries, I use a simple exponential backoff rather than pulling in a retry crate:

```rust
async fn fetch_with_retry(
    client: &reqwest::Client,
    url: &str,
    max_attempts: u32,
) -> Result<String, reqwest::Error> {
    let mut attempt = 0;
    loop {
        match client.get(url).send().await {
            Ok(response) => return response.text().await,
            Err(e) if attempt < max_attempts => {
                attempt += 1;
                let delay = std::time::Duration::from_millis(
                    500 * 2u64.pow(attempt),
                );
                tokio::time::sleep(delay).await;
            }
            Err(e) => return Err(e),
        }
    }
}
```

This retries up to `max_attempts` times with delays of 1s, 2s, 4s. It only retries on connection-level errors. HTTP errors (4xx, 5xx) are returned immediately because retrying a bad request or a server error is usually pointless without user intervention.

## Streaming large responses

For downloading files or streaming data, reading the entire response into memory is wasteful. reqwest supports byte streams:

```rust
use tokio::io::AsyncWriteExt;

async fn download_file(
    client: &reqwest::Client,
    url: &str,
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut response = client.get(url).send().await?;
    let mut file = tokio::fs::File::create(path).await?;

    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk).await?;
    }

    Ok(())
}
```

Each `chunk()` call returns the next batch of bytes as they arrive. You can also use `response.bytes_stream()` from the `stream` feature for more control over the stream, including applying timeouts to individual chunks.

## Wrapping up

reqwest handles the routine parts of HTTP so you can focus on your app. Set up a client once, reuse it everywhere, handle errors explicitly, and use serde for typed responses. Those four patterns cover most of what a desktop app needs from an HTTP client.

If you are building a desktop app with GPUI and want a starting point that already has the async plumbing, project structure, and common patterns wired up, take a look at [gpui-starter](/docs/getting-started/). It includes request examples, error boundaries, and a full app scaffold so you can skip the boilerplate and start building features. The source is on [GitHub](https://github.com/freeoxide/gpui-starter).
