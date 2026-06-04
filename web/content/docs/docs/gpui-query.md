---
title: gpui-query: Async Data Fetching
description: Fetch, cache, and sync async data in GPUI with stale-while-revalidate and automatic refetching.
---

GPUI renders synchronously. Every frame, `render` reads state from entities and produces an element tree. There is no built-in mechanism for fetching data from a network, disk, or background task. You end up writing the same boilerplate each time: create an entity, spawn a task, update on completion, handle errors, handle cancellation when the user navigates away.

`gpui-query` extracts that pattern into a reusable crate inspired by [TanStack Query v5](https://tanstack.com/query), adapted for GPUI's synchronous rendering model and Rust's ownership rules. Pass a key and a fetcher closure. The crate manages the entity, cache policy, retry logic, cancellation, and re-render scheduling, so you do not have to write state machines for each component.

## The three layers

The crate is split into feature flags. Lower layers have zero GPUI coupling.

| Layer | Feature | Depends on | Purpose |
|-------|---------|------------|---------|
| Core | `core` | `serde` only | State machine (`QueryResource`, `CachePolicy`, `RetryPolicy`) |
| Client | `client` | `core` + GPUI | `QueryClient` global registry with type-partitioned buckets |
| Hook | `hook` | `client` | `use_query`, `use_mutation`, `use_infinite_query` |

The `default` feature enables `client`. Add `hook` for the full ergonomic API.

```toml
[dependencies]
gpui-query-v2 = { path = "crates/gpui-query-v2", features = ["hook"] }
```

## Quick start

Initialize `QueryClient` as a GPUI global during app setup.

```rust
// main.rs or app initialization
use gpui_query_v2::QueryClient;

cx.set_global(QueryClient::new());
```

Then call `use_query` in your component's constructor. The hook returns a tuple of `(Entity<QueryResource<T, E>>, Subscription)`.

```rust
use gpui_query_v2::{use_query, QueryOptions, QueryResource, CachePolicy};
use gpui_query_v2::hook::QueryOptions;

struct UserList {
    users: gpui::Entity<QueryResource<Vec<User>, MyError>>,
    _sub: gpui::Subscription,
}

impl UserList {
    fn new(cx: &mut gpui::Context<Self>) -> Self {
        let (users, _sub) = use_query(
            QueryOptions::new("users")
                .cache_policy(CachePolicy::Ttl { ttl_ms: 60_000 }),
            |signal| async move {
                // signal.is_cancelled() lets you abort early
                let response = fetch_users().await?;
                Ok(response)
            },
            cx,
        );
        Self { users, _sub }
    }
}
```

During `render`, read the entity state:

```rust
fn render(&mut self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
    let resource = self.users.read(cx);

    match resource.status() {
        QueryStatus::LoadingEmpty | QueryStatus::Idle => div().child("Loading..."),
        QueryStatus::Failure => div().child(format!("Error: {:?}", resource.error())),
        QueryStatus::Success => {
            let data = resource.data().unwrap();
            div().children(data.iter().map(|u| div().child(u.name.clone())))
        }
        _ => div(),
    }
}
```

The `Subscription` keeps the observer alive. Store it as a field. Dropping it unsubscribes and the resource becomes eligible for garbage collection.

## Mutations

Write operations (create, update, delete) use `use_mutation`. Call `mutate` from event handlers.

```rust
use gpui_query_v2::hook::{use_mutation, mutate};

struct MyView {
    create_user: gpui::Entity<MutationResource<NewUser, User, MyError>>,
    _sub: gpui::Subscription,
}

impl MyView {
    fn new(cx: &mut gpui::Context<Self>) -> Self {
        let (entity, sub) = use_mutation((), cx);
        Self { create_user: entity, _sub: sub }
    }

    fn handle_submit(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        mutate(&self.create_user, NewUser { name }, |vars| async move {
            api_create_user(vars).await
        }, cx);
    }
}
```

Mutations support lifecycle callbacks via `mutate_with_callbacks`. The `on_success`, `on_error`, and `on_settled` closures fire on the final outcome after all retries are exhausted.

## Paginated data with use_infinite_query

For scrollable lists that load in pages, `use_infinite_query` manages a `VecDeque` of page data with bounded memory.

```rust
use gpui_query_v2::hook::{
    use_infinite_query, fetch_next_page_infinite,
    InfiniteQueryOptions,
};

let (entity, sub) = use_infinite_query(
    InfiniteQueryOptions::new("posts")
        .max_pages(20),
    // first page fetcher
    |signal| async move { fetch_posts_page(None).await },
    cx,
);
```

Call `fetch_next_page_infinite(&entity, fetcher, cx)` to load the next page. The resource tracks `has_next_page` and `has_previous_page` automatically.

## Cache policies

`CachePolicy` controls when cached data is served without refetching.

| Policy | Behavior |
|--------|----------|
| `NoCache` | Always fetch. No cache storage. |
| `Ttl { ttl_ms }` | Serve cached data within the TTL window. Refetch on expiry. |
| `StaleWhileRevalidate { ttl_ms, stale_ms }` | Serve fresh data within TTL. Serve stale data and background-revalidate within `ttl_ms + stale_ms`. |

Default is `Ttl { ttl_ms: 60_000 }` (one minute).

## Retry policy

`RetryPolicy` configures automatic retries on fetch failure.

```rust
use gpui_query_v2::RetryPolicy;

let policy = RetryPolicy::new(3)          // max 3 retries
    .with_delay(500)                      // 500ms base delay
    .with_exponential_backoff()           // 500ms, 1s, 2s...
    .with_max_delay(10_000);             // cap at 10s
```

Default is 3 retries with exponential backoff, 1-second base, 30-second cap. Mutations default to no retries.

## Request policy

`RequestPolicy` controls concurrent fetch behavior for a single key.

- **`LatestWins`** (default): A new request cancels the in-flight one. The fetcher receives a cancelled signal.
- **`IgnoreWhileLoading`**: New requests are dropped while one is active. Useful for preventing duplicate form submissions.

## Query keys

`QueryKey` is a hierarchical array of strings, like TanStack Query's `["users", "42"]`.

```rust
use gpui_query_v2::QueryKey;

let key = QueryKey::from(["users", "42", "posts"]);
assert!(key.starts_with(&QueryKey::from(["users"])));
```

Internally uses `Arc<[Arc<str>]>` so cloning is cheap regardless of key length. Keys are `Serialize`/`Deserialize` for persistence.

## Invalidation and cache control

`QueryClient` provides bulk operations matching TanStack Query's client API.

```rust
use gpui_query_v2::{QueryClient, QueryKeyFilter};

// Invalidate all queries starting with "users"
client.invalidate_queries(&QueryKeyFilter::prefix("users"), cx);

// Cancel in-flight requests matching a prefix
client.cancel_queries(&QueryKeyFilter::prefix("temp"), cx);

// Reset queries to Idle (clears data)
client.reset_queries(&QueryKeyFilter::all(), cx);

// Direct cache read/write
let data: Option<Vec<User>> = client.get_query_data(&key, cx);
client.set_query_data("users", my_users, cx);

// Garbage collection (runs on idle resources past gc_time_ms)
client.gc(cx);
```

`QueryKeyFilter` supports `Exact`, `Prefix`, and `All` matching. Set a custom GC time with `QueryClient::new().with_gc_time(600_000)` (default is 5 minutes).

## Cooperative cancellation

Every fetcher receives a `QuerySignal` (in v2, this is always provided). The signal uses a shared atomic flag. When `LatestWins` replaces a request, the old signal is cancelled.

```rust
let (entity, sub) = use_query(
    QueryOptions::new("search"),
    |signal| async move {
        // Periodically check in your fetcher
        let chunk = fetch_chunk().await;
        if signal.is_cancelled() {
            return Err(MyError::Cancelled);
        }
        // continue...
        Ok(data)
    },
    cx,
);
```

## Imperative fetching

For one-shot fetches outside a component (background tasks, app startup), use `prepare_fetch_query`.

```rust
if let Some(prepared) = client.prepare_fetch_query::<UserData, MyError>(
    QueryKey::from("user/42"), cx,
) {
    let signal = prepared.signal.clone();
    cx.spawn(async move |_this, cx| {
        let data = fetch_user(42, signal).await;
        prepared.complete_success(data?, cx);
        Ok::<_, ()>(())
    }).detach();
}
```

`prepare_prefetch_query` works the same but respects cache policy. If data is fresh, it returns `None`.

## Persistence across restarts

Implement `QueryPersister` to serialize cached data to disk.

```rust
use gpui_query_v2::client::QueryPersister;

struct FilePersister { path: PathBuf }

impl QueryPersister for FilePersister {
    fn load(&self) -> Vec<DehydratedEntry> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, entries: Vec<DehydratedEntry>) {
        if let Ok(json) = serde_json::to_string(&entries) {
            let _ = std::fs::write(&self.path, json);
        }
    }
}
```

Call `client.persist(&persister, cx)` on shutdown and restore on startup.

## Configuration reference

| Option | Default | Description |
|--------|---------|-------------|
| `CachePolicy` | `Ttl { ttl_ms: 60_000 }` | How long cached data is considered fresh |
| `RequestPolicy` | `LatestWins` | How concurrent requests for the same key interact |
| `RetryPolicy` (queries) | 3 retries, exponential backoff, 1s base, 30s cap | Retry behavior on fetch failure |
| `RetryPolicy` (mutations) | No retries | Retry behavior for mutations |
| `gc_time_ms` | 300,000 (5 minutes) | How long idle resources survive before GC |
| `max_pages` (infinite) | 50 | Maximum pages retained in an infinite query |

## Next steps

- [Architecture](/docs/architecture/) for how entities, globals, and subscriptions work in GPUI
- [Testing](/docs/testing/) for writing tests against components that use query hooks
- [Getting started with GPUI](/blog/getting-started-with-gpui/) for framework fundamentals
- [Building desktop apps with Rust and GPUI](/blog/building-desktop-apps-rust-gpui/) for a broader project walkthrough
