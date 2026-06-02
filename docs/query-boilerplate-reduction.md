# gpui-query — Boilerplate Reduction Plan

**Goal:** Reduce the most common operations to absolute zero boilerplate, matching TanStack Query's ergonomics in a GPUI-native way.

---

## The Problem: Current vs TanStack

### Simple Query

**TanStack Query (3 lines):**
```tsx
const users = useQuery({
  queryKey: ['users'],
  queryFn: fetchUsers,
});
```

**gpui-query current (12+ lines):**
```rust
struct MyView {
    users: Entity<QueryResource<Vec<User>>>,
    _users_sub: Subscription,
}

impl MyView {
    fn new(cx: &mut Context<Self>) -> Self {
        let (users, _users_sub) = use_query(
            QueryKey::from(["users"]),
            CachePolicy::Ttl { ttl_ms: 60_000 },
            RequestPolicy::LatestWins,
            || async { fetch_users().await.map_err(|e| QueryError::unknown(e.to_string())) },
            cx,
        );
        Self { users, _users_sub }
    }
}
```

**gpui-query target (5 lines):**
```rust
struct MyView {
    users: UseQueryResult<Vec<User>>,
}

impl MyView {
    fn new(cx: &mut Context<Self>) -> Self {
        let users = use_query("users", || async { fetch_users().await }, cx);
        Self { users }
    }
}
```

**Lines reduced:** 12 → 5 (58% reduction)

---

### Mutation with Callbacks

**TanStack Query (6 lines):**
```tsx
const addTodo = useMutation({
  mutationFn: (todo) => api.addTodo(todo),
  onSuccess: () => queryClient.invalidateQueries({ queryKey: ['todos'] }),
});
```

**gpui-query current (10 lines):**
```rust
let add_todo = use_mutation::<NewTodo, Todo, QueryError, Self>(cx);
// Later, in event handler:
let callbacks = MutationCallbacks::new()
    .on_success(|_data| { /* invalidate */ });
mutate_with_callbacks(
    &add_todo,
    new_todo,
    |vars| async { api.add_todo(vars).await },
    callbacks,
    cx,
);
```

**gpui-query target (4 lines):**
```rust
let add_todo = use_mutation(|vars: NewTodo| async { api.add_todo(vars).await }, cx);
// Later:
add_todo.mutate(new_todo, cx).on_success(|_| { /* invalidate */ });
```

**Lines reduced:** 10 → 4 (60% reduction)

---

### Infinite Query

**TanStack Query (8 lines):**
```tsx
const { data, fetchNextPage, hasNextPage } = useInfiniteQuery({
  queryKey: ['posts'],
  queryFn: ({ pageParam }) => fetchPosts(pageParam),
  initialPageParam: 0,
  getNextPageParam: (lastPage) => lastPage.nextCursor,
});
```

**gpui-query current (15+ lines):**
```rust
struct Feed {
    posts: Entity<InfiniteQueryResource<Vec<Post>>>,
    _posts_sub: Subscription,
}

impl Feed {
    fn new(cx: &mut Context<Self>) -> Self {
        let (posts, _fetch_next, _posts_sub) = use_infinite_query(
            InfiniteQueryOptions::new(QueryKey::from(["posts"])),
            |last_page| async move {
                let cursor = last_page.and_then(|p| p.last().and_then(|i| i.cursor));
                fetch_posts(cursor).await
            },
            cx,
        );
        // _fetch_next is a NO-OP — must use fetch_next_page_infinite separately!
        Self { posts, _posts_sub }
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        fetch_next_page_infinite(
            &self.posts,
            |last_page| async move {
                let cursor = last_page.and_then(|p| p.last().and_then(|i| i.cursor));
                fetch_posts(cursor).await
            },
            cx,
        );
    }
}
```

**gpui-query target (9 lines):**
```rust
struct Feed {
    posts: UseInfiniteQueryResult<Vec<Post>>,
}

impl Feed {
    fn new(cx: &mut Context<Self>) -> Self {
        let posts = use_infinite_query(
            "posts",
            |last_page| async { fetch_posts(last_page.cursor()).await },
            cx,
        );
        Self { posts }
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        self.posts.fetch_next(cx);
    }
}
```

**Lines reduced:** 15 → 9 (40% reduction)

---

## Detailed Changes

### Change 1: `QueryOptions`-First API

**Files:** `hook/mod.rs`, `hook/options.rs`

```rust
// hook/options.rs — enhanced QueryOptions with From impls

impl<T, E> QueryOptions<T, E> {
    /// Create options with just a key. Everything else uses defaults.
    pub fn new(key: impl Into<QueryKey>) -> Self {
        Self {
            key: key.into(),
            ..Default::default()
        }
    }
}

impl<T, E> Default for QueryOptions<T, E> {
    fn default() -> Self {
        Self {
            key: QueryKey::from(["default"]),
            cache_policy: CachePolicy::default(),    // Ttl { ttl_ms: 60_000 }
            request_policy: RequestPolicy::default(), // LatestWins
            gc_time_ms: 300_000,
            force_fetch: false,
            keep_previous_data: false,
            initial_data: None,
            retry_policy: RetryPolicy::default(),    // 3 retries, exponential
            network_mode: NetworkMode::default(),    // Online
            refetch_on_mount: RefetchTrigger::default(), // OnMount
        }
    }
}

// Allow passing a &str or QueryKey directly:
impl<T, E> From<&str> for QueryOptions<T, E> {
    fn from(key: &str) -> Self { Self::new(key) }
}

impl<T, E> From<QueryKey> for QueryOptions<T, E> {
    fn from(key: QueryKey) -> Self { Self::new(key) }
}
```

**Hook signature change:**

```rust
/// Primary query hook. Accepts a key (or QueryOptions), a fetcher, and context.
///
/// # Quick Start
///
/// ```ignore
/// let users = use_query("users", || async { fetch_users().await }, cx);
///
/// // With options:
/// let users = use_query(
///     QueryOptions::new("users")
///         .cache_policy(CachePolicy::Ttl { ttl_ms: 5 * 60_000 })
///         .retry_policy(RetryPolicy::max_attempts(5)),
///     || async { fetch_users().await },
///     cx,
/// );
/// ```
pub fn use_query<T, E, O, C, F, Fut>(
    options: O,
    fetcher: F,
    cx: &mut Context<C>,
) -> UseQueryResult<T, E>
where
    T: 'static,
    E: 'static,
    O: Into<QueryOptions<T, E>>,
    C: 'static,
    F: Fn(QuerySignal) -> Fut + 'static,
    Fut: Future<Output = Result<T, E>> + 'static,
```

**Boilerplate eliminated:**
- ✅ No need to specify `CachePolicy` when defaults work
- ✅ No need to specify `RequestPolicy` when defaults work
- ✅ Retry, initial_data, keep_previous_data now accessible
- ✅ `QueryKey` construction simplified (just pass a string)

---

### Change 2: Named Result Structs

**Files:** New structs in `hook/mod.rs` (or `hook/result.rs`)

```rust
/// Result of `use_query`. Holds the reactive entity and observation subscription.
///
/// Store this in your component struct. The subscription keeps the observation
/// alive — when your component drops, the subscription drops too.
pub struct UseQueryResult<T, E = QueryError> {
    pub entity: Entity<QueryResource<T, E>>,
    pub(crate) subscription: Subscription,
}

impl<T: Clone + 'static, E: 'static> UseQueryResult<T, E> {
    /// Current data, if loaded. Clones the data.
    pub fn data(&self, cx: &App) -> Option<T> {
        self.entity.read(cx).data().cloned()
    }

    /// Display data (real data, or placeholder/initial if available).
    pub fn display_data(&self, cx: &App) -> Option<T> {
        self.entity.read(cx).display_data().cloned()
    }

    /// Is the query currently loading (first time or refetch)?
    pub fn is_loading(&self, cx: &App) -> bool {
        self.entity.read(cx).is_loading()
    }

    /// Is this the initial load (no data yet)?
    pub fn is_pending(&self, cx: &App) -> bool {
        matches!(self.entity.read(cx).status(), QueryStatus::LoadingEmpty)
    }

    /// Current error, if any.
    pub fn error(&self, cx: &App) -> Option<&E> {
        self.entity.read(cx).error()
    }

    /// Current query status.
    pub fn status(&self, cx: &App) -> QueryStatus {
        self.entity.read(cx).status()
    }

    /// Imperatively refetch the query.
    pub fn refetch<F, Fut>(&self, fetcher: F, cx: &mut Context<C>) where
        C: 'static,
        F: Fn(QuerySignal) -> Fut + 'static,
        Fut: Future<Output = Result<T, E>> + 'static,
    {
        fetch_query_with_signal(&self.entity, fetcher, cx);
    }
}
```

**Boilerplate eliminated:**
- ✅ No more tuple destructuring (`let (entity, _sub) = ...`)
- ✅ Convenience methods avoid `entity.read(cx).data().cloned()` pattern
- ✅ Subscription stored internally — one fewer field in component struct

---

### Change 3: Mutation Overhaul

**Files:** `hook/mod.rs`

```rust
/// Create a mutation hook. The mutator function is captured for future calls.
///
/// ```ignore
/// let add_todo = use_mutation(
///     |vars: NewTodo| async { api.add_todo(vars).await },
///     cx,
/// );
///
/// // Trigger mutation:
/// add_todo.mutate(new_todo, cx);
///
/// // With callbacks:
/// add_todo.mutate(new_todo, cx)
///     .on_success(|todo| println!("Created: {todo:?}"))
///     .on_error(|err| eprintln!("Failed: {err:?}"));
/// ```
pub fn use_mutation<V, T, E, C, F, Fut>(
    mutator: F,
    cx: &mut Context<C>,
) -> UseMutationResult<V, T, E>
where
    V: 'static,
    T: 'static,
    E: 'static,
    C: 'static,
    F: Fn(V) -> Fut + 'static + Clone,
    Fut: Future<Output = Result<T, E>> + 'static,
{
    let entity: Entity<MutationResource<V, T, E>> = cx.new(|_| MutationResource::new());
    UseMutationResult {
        entity,
        mutator: Arc::new(move |v| {
            let f = mutator.clone();
            Box::pin(async move { f(v).await })
        }),
    }
}

pub struct UseMutationResult<V, T, E> {
    pub entity: Entity<MutationResource<V, T, E>>,
    mutator: Arc<dyn Fn(V) -> Pin<Box<dyn Future<Output = Result<T, E>> + Send>> + Send + Sync>,
}

/// Builder for a single mutation execution. Call .on_success/.on_error to add callbacks.
pub struct MutationExecution {
    // internal state for callback registration
}

impl MutationExecution {
    pub fn on_success<F: Fn(&T) + 'static>(self, callback: F) -> Self { ... }
    pub fn on_error<F: Fn(&E) + 'static>(self, callback: F) -> Self { ... }
    pub fn on_settled<F: Fn(Option<&T>, Option<&E>) + 'static>(self, callback: F) -> Self { ... }
}

impl<V, T, E> UseMutationResult<V, T, E> {
    /// Trigger a mutation with the given variables.
    pub fn mutate<C: 'static>(&self, variables: V, cx: &mut Context<C>) -> MutationExecution { ... }
}
```

**Boilerplate eliminated:**
- ✅ Mutator function defined once at hook creation (not re-passed each time)
- ✅ Inline callbacks instead of separate `MutationCallbacks` struct
- ✅ Builder pattern for callbacks (`.on_success().on_error()`)
- ✅ No need to import `MutationCallbacks` separately

---

### Change 4: Infinite Query Fix

**Files:** `hook/use_infinite_query.rs`

```rust
pub fn use_infinite_query<T, E, C, FNext, Fut>(
    options: impl Into<InfiniteQueryOptions<T, E>>,
    fetch_next: FNext,
    cx: &mut Context<C>,
) -> UseInfiniteQueryResult<T, E>
where
    T: Send + 'static,
    E: Send + 'static,
    C: 'static,
    FNext: Fn(Option<&T>) -> Fut + 'static + Clone + Send + Sync,
    Fut: Future<Output = Result<T, E>> + Send + 'static,
{
    // ... create entity, register with client, start initial fetch ...

    UseInfiniteQueryResult {
        entity,
        subscription,
        fetcher_next: Arc::new(move |last_page: Option<&T>| {
            let f = fetch_next.clone();
            Box::pin(async move { f(last_page).await })
        }),
        fetcher_prev: None,
    }
}

impl<T, E> UseInfiniteQueryResult<T, E> {
    /// Fetch the next page. No-op if already fetching or no more pages.
    pub fn fetch_next<C: 'static>(&self, cx: &mut Context<C>) { ... }

    /// Fetch the previous page. No-op if already fetching or no previous page.
    pub fn fetch_previous<C: 'static>(&self, cx: &mut Context<C>) { ... }

    /// All loaded pages.
    pub fn pages(&self, cx: &App) -> &[T] { self.entity.read(cx).pages() }

    /// Convenience: flatten all pages into a single iterator.
    pub fn items<U>(&self, cx: &App) -> Vec<U>
    where T: AsRef<[U]>, U: Clone {
        self.pages(cx).iter().flat_map(|p| p.as_ref().iter().cloned()).collect()
    }
}
```

**Boilerplate eliminated:**
- ✅ `fetch_next(cx)` instead of re-providing the fetcher closure
- ✅ No dead closure in return type
- ✅ Fetcher stored internally — defined once at hook creation

---

### Change 5: QueryClient Setup

**Files:** `client/mod.rs`

```rust
// Before:
cx.set_global(QueryClient::new(
    CachePolicy::default(),
    RequestPolicy::default(),
));

// After:
cx.set_global(QueryClient::default());
// Or even shorter:
cx.default_global::<QueryClient>(); // GPUI idiom for set_global(T::default())
```

---

## Impact Summary

| Operation | Before | After | Reduction |
|-----------|--------|-------|-----------|
| Simple query | 12 lines | 5 lines | **58%** |
| Query with options | 14 lines | 7 lines | **50%** |
| Mutation | 10 lines | 4 lines | **60%** |
| Infinite query | 15 lines | 9 lines | **40%** |
| Client setup | 3 lines | 1 line | **67%** |

**Average boilerplate reduction: ~55%**

### What Users No Longer Need to Do:

- ❌ ~~Specify `CachePolicy` and `RequestPolicy` on every `use_query` call~~
- ❌ ~~Destructure tuples from hooks~~
- ❌ ~~Store `_subscription` as a separate field~~
- ❌ ~~Re-provide the fetcher to `fetch_next_page_infinite`~~
- ❌ ~~Create `MutationCallbacks` struct for simple callbacks~~
- ❌ ~~Import `RequestSequencer` for direct resource usage~~
- ❌ ~~Pass `CachePolicy::default(), RequestPolicy::default()` to `QueryClient::new()`~~

### What Users Still Need to Do (Can't Be Eliminated):

- ✅ Store the result struct in their component (Rust ownership requirement)
- ✅ Provide the async fetcher function (essential logic)
- ✅ Call `cx.notify()` if manually mutating the entity outside hooks
