# gpui-query

Async state management for [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), modeled after [TanStack Query](https://tanstack.com/query).

GPUI draws everything synchronously on the main thread. That makes async data awkward: you end up hand-rolling loading states, error handling, caching, deduplication, retries, and cancellation. gpui-query handles those pieces.

You write a fetcher. The library manages the lifecycle.

## Install

```toml
[dependencies]
gpui-query = "0.1.4"
```

The default feature set includes the `client` layer. To use the declarative view hooks, enable the `hook` feature:

```toml
[dependencies]
gpui-query = { version = "0.1.4", features = ["hook"] }
```

If you only want the core state machine without pulling in GPUI:

```toml
[dependencies]
gpui-query = { version = "0.1.4", default-features = false, features = ["core"] }
```

## Quick start

Set up a `QueryClient` as a GPUI global when your app starts:

```rust
use gpui::App;
use gpui_query::QueryClient;

App::new().run(|cx| {
    cx.set_global(QueryClient::new());
    // ... your views
});
```

Create a query in your view:

```rust
use gpui_query::{use_query, QueryOptions};

fn setup_query(cx: &mut ViewContext<MyView>) -> (Entity<QueryResource<Vec<User>, MyError>>, Subscription) {
    use_query(
        "users",
        |signal| async move {
            let users = fetch_users().await?;
            Ok::<Vec<User>, MyError>(users)
        },
        cx,
    )
}
```

Read the state in `render`:

```rust
fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
    let entity = self.query_entity.clone();
    entity.read_with(cx, |resource| {
        match resource.status() {
            QueryStatus::LoadingEmpty => "Loading...",
            QueryStatus::Success => "Got data",
            QueryStatus::Failure => "Error",
            _ => "Idle",
        }
    })
}
```

## Feature layers

The crate is split into three layers, each behind a feature flag:

- `core` is a serde-only state machine with no framework coupling. `QueryResource`, `MutationResource`, `CachePolicy`, `RetryPolicy`, and `QueryKey` live here. You can use this layer in any Rust project.
- `client` is the default feature. It adds `QueryClient`, a GPUI `Global` that owns type-partitioned storage. It handles garbage collection, cache invalidation, observers, persistence, and devtools diagnostics.
- `hook` provides the declarative hooks (`use_query`, `use_mutation`, `use_infinite_query`) that wire the client into GPUI views. All hooks return `(Entity, Subscription)` tuples.

## What you get

- Caching with `NoCache`, `Ttl`, and `StaleWhileRevalidate` policies.
- Deduplication of concurrent requests that share a key.
- Retry with configurable exponential backoff.
- Cooperative cancellation through the `QuerySignal` passed to every fetcher.
- Garbage collection of idle resources after a configurable TTL.
- Cache invalidation by exact key, prefix, or globally.
- Optimistic updates with rollback support.
- Mutation callbacks for success, error, and settled states.
- Infinite queries for paginated data.
- Error sanitization that strips connection strings, tokens, paths, emails, and hex keys from messages.
- Persistence through the `QueryPersister` trait.

## Links

- Website: <https://gpui-query.hmziq.xyz>
- Docs: <https://gpui-query.hmziq.xyz/docs/>
- Source: <https://github.com/hmziqrs/gpui-query>
- GPUI: <https://github.com/zed-industries/zed/tree/main/crates/gpui>
- TanStack Query: <https://tanstack.com/query>

## Author

**hmziqrs**

- Website: <https://hmziq.rs>
- GitHub: <https://github.com/hmziqrs>
- X: <https://x.com/hmziqrs>

## License

MIT. See the [LICENSE](../../LICENSE) file for details.

