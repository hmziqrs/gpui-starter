//! The `use_query` and `use_mutation` hooks — ergonomic query and mutation
//! subscriptions for GPUI components.
//!
//! # Query Usage
//!
//! ```ignore
//! use gpui_query::hook::use_query;
//! use gpui_query::{CachePolicy, QueryKey, RequestPolicy};
//!
//! struct MyView {
//!     users: gpui::Entity<gpui_query::QueryResource<Vec<User>>>,
//!     _subscription: gpui::Subscription,
//! }
//!
//! impl MyView {
//!     fn new(cx: &mut gpui::Context<Self>) -> Self {
//!         let (users, _subscription) = use_query(
//!             QueryKey::from(["users"]),
//!             CachePolicy::Ttl { ttl_ms: 60_000 },
//!             RequestPolicy::LatestWins,
//!             || async {
//!                 let resp = reqwest::get("/api/users").await?;
//!                 let users: Vec<User> = resp.json().await?;
//!                 Ok(users)
//!             },
//!             cx,
//!         );
//!         Self { users, _subscription }
//!     }
//! }
//! ```
//!
//! # Mutation Usage
//!
//! ```ignore
//! use gpui_query::hook::{use_mutation, mutate};
//!
//! struct MyView {
//!     create_user: gpui::Entity<gpui_query::MutationResource<NewUser, User>>,
//! }
//!
//! impl MyView {
//!     fn new(cx: &mut gpui::Context<Self>) -> Self {
//!         let entity = use_mutation(cx);
//!         Self { create_user: entity }
//!     }
//!
//!     fn handle_submit(&mut self, name: String, cx: &mut gpui::Context<Self>) {
//!         mutate(&self.create_user, NewUser { name }, |vars| async move {
//!             api::create_user(&vars).await
//!         }, cx);
//!     }
//! }
//! ```

mod options;
mod use_infinite_query;

pub use options::{MutationCallbacks, MutationOptions, QueryOptions};
pub use use_infinite_query::{
    fetch_next_page_infinite, fetch_previous_page_infinite, InfiniteQueryOptions,
    use_infinite_query,
};

use gpui::{AppContext, BorrowAppContext as _, Context, Entity, Subscription};

use crate::client::QueryClient;
use crate::core::{
    MutationResource, QueryKey, QueryResource, QuerySignal, QueryStatus, RetryPolicy,
};

// ── Query hooks ─────────────────────────────────────────────────────────

/// Subscribe to a query resource and automatically re-render when it changes.
///
/// Call this in your component's constructor (not in `render`). It:
///
/// 1. Gets or creates a [`QueryResource`] entity from the global [`QueryClient`]
/// 2. Calls `cx.observe()` so your component re-renders on state changes
/// 3. Starts an async fetch if the resource is idle
/// 4. On failure, retries according to the resource's retry policy
///
/// # Returns
///
/// A tuple of `(Entity<QueryResource<T, E>>, Subscription)`:
/// - Store the entity to read state during render
/// - Store the subscription to keep the observation alive
pub fn use_query<T, E, C, F, Fut>(
    key: QueryKey,
    cache_policy: crate::core::CachePolicy,
    request_policy: crate::core::RequestPolicy,
    fetcher: F,
    cx: &mut Context<C>,
) -> (Entity<QueryResource<T, E>>, Subscription)
where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let (entity, subscription) = use_query_manual(key, cache_policy, request_policy, cx);

    // Start fetch if resource is idle
    let should_fetch = entity.read_with(cx, |r, _| r.status() == QueryStatus::Idle);
    if should_fetch {
        let weak = entity.downgrade();
        let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
        cx.spawn(async move |_this, cx| {
            fetch_with_retry(&fetcher, &retry_policy, &weak, cx).await;
            Ok::<_, ()>(())
        })
        .detach();
    }

    (entity, subscription)
}

/// Like [`use_query`], but the fetcher receives a [`QuerySignal`] that it can
/// check periodically for cooperative cancellation.
///
/// The fetcher signature is `Fn(QuerySignal) -> Fut` instead of `Fn() -> Fut`.
/// On failure, retries according to the resource's retry policy by creating
/// a fresh signal for each retry attempt.
pub fn use_query_with_signal<T, E, C, F, Fut>(
    key: QueryKey,
    cache_policy: crate::core::CachePolicy,
    request_policy: crate::core::RequestPolicy,
    fetcher: F,
    cx: &mut Context<C>,
) -> (Entity<QueryResource<T, E>>, Subscription)
where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    F: Fn(QuerySignal) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let (entity, subscription) = use_query_manual(key, cache_policy, request_policy, cx);

    // Start fetch if resource is idle
    let should_fetch = entity.read_with(cx, |r, _| r.status() == QueryStatus::Idle);
    if should_fetch {
        let signal = entity.read_with(cx, |r, _| {
            r.signal().cloned().unwrap_or_else(QuerySignal::new)
        });
        let weak = entity.downgrade();
        let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
        cx.spawn(async move |_this, cx| {
            fetch_signal_with_retry(&fetcher, signal, &retry_policy, &weak, cx).await;
            Ok::<_, ()>(())
        })
        .detach();
    }

    (entity, subscription)
}

/// Lower-level hook that sets up the entity and observation without starting a fetch.
///
/// Use this when you need full control over when and how fetching happens.
pub fn use_query_manual<T, E, C>(
    key: QueryKey,
    cache_policy: crate::core::CachePolicy,
    request_policy: crate::core::RequestPolicy,
    cx: &mut Context<C>,
) -> (Entity<QueryResource<T, E>>, Subscription)
where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
    C: 'static,
{
    let entity = if cx.has_global::<QueryClient>() {
        cx.update_global::<QueryClient, _>(|client, cx| {
            client.resource_with_policies::<T, E>(key, cache_policy, request_policy, cx)
        })
    } else {
        cx.new(|_| QueryResource::new(key, cache_policy, request_policy))
    };

    let subscription = cx.observe(&entity, |_, _, cx| {
        cx.notify();
    });

    (entity, subscription)
}

/// Initiate a fetch on an existing query entity.
///
/// Call this when you want to refetch (e.g., on button click or timer).
/// Respects the resource's retry policy on failure.
pub fn fetch_query<T, E, C, F, Fut>(
    entity: &Entity<QueryResource<T, E>>,
    fetcher: F,
    cx: &mut Context<C>,
) where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let weak = entity.downgrade();
    let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
    cx.spawn(async move |_this, cx| {
        fetch_with_retry(&fetcher, &retry_policy, &weak, cx).await;
        Ok::<_, ()>(())
    })
    .detach();
}

/// Like [`fetch_query`], but the fetcher receives a [`QuerySignal`] that it can
/// check periodically for cooperative cancellation.
///
/// The fetcher signature is `FnOnce(QuerySignal) -> Fut` instead of `FnOnce() -> Fut`.
/// Note: FnOnce fetchers cannot be retried because the closure is consumed on the first call.
pub fn fetch_query_with_signal<T, E, C, F, Fut>(
    entity: &Entity<QueryResource<T, E>>,
    fetcher: F,
    cx: &mut Context<C>,
) where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    F: FnOnce(QuerySignal) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let signal = entity.read_with(cx, |r, _| {
        r.signal().cloned().unwrap_or_else(QuerySignal::new)
    });
    let weak = entity.downgrade();

    // FnOnce fetchers can only be called once, so retries are not possible.
    cx.spawn(async move |_this, cx| {
        let result = fetcher(signal).await;
        let now_ms = current_time_ms();
        let entity = match weak.upgrade() {
            Some(e) => e,
            None => return Ok::<_, ()>(()),
        };
        entity.update(cx, |resource, cx| {
            if let Some(guard) = resource.accept_current_request(
                resource
                    .active_request_id()
                    .unwrap_or(crate::core::RequestId::scoped(0, 0)),
            ) {
                match result {
                    Ok(data) => {
                        resource.complete_success(&guard, data, now_ms);
                    }
                    Err(error) => {
                        resource.complete_failure(&guard, error);
                    }
                }
            }
            cx.notify();
        });
        Ok::<_, ()>(())
    })
    .detach();
}

// ── Retry-aware fetch helpers ───────────────────────────────────────────

/// Execute a fetch with retry logic for a query resource.
///
/// Calls the fetcher. On failure, if the retry policy allows it, waits for the
/// configured delay and retries. Updates the entity state between attempts.
/// Resets the retry counter on success.
async fn fetch_with_retry<T, E, F, Fut>(
    fetcher: &F,
    retry_policy: &RetryPolicy,
    entity: &gpui::WeakEntity<QueryResource<T, E>>,
    cx: &mut gpui::AsyncApp,
) where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let mut attempt: u32 = 0;

    loop {
        let result = fetcher().await;

        match result {
            Ok(data) => {
                let now_ms = current_time_ms();
                let entity = match entity.upgrade() {
                    Some(e) => e,
                    None => return,
                };
                entity.update(cx, |resource, cx| {
                    resource.reset_retry_count();
                    if let Some(guard) = resource.accept_current_request(
                        resource
                            .active_request_id()
                            .unwrap_or(crate::core::RequestId::scoped(0, 0)),
                    ) {
                        resource.complete_success(&guard, data, now_ms);
                    }
                    cx.notify();
                });
                return;
            }
            Err(error) => {
                if retry_policy.should_retry(attempt) {
                    let delay_ms = retry_policy.delay_for_attempt(attempt);
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    e.update(cx, |resource, cx| {
                        resource.increment_retry();
                        cx.notify();
                    });
                    attempt += 1;

                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(delay_ms))
                            .await;
                    }
                    // Loop to retry
                } else {
                    // No more retries — complete with failure
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    e.update(cx, |resource, cx| {
                        if let Some(guard) = resource.accept_current_request(
                            resource
                                .active_request_id()
                                .unwrap_or(crate::core::RequestId::scoped(0, 0)),
                        ) {
                            resource.complete_failure(&guard, error);
                        }
                        cx.notify();
                    });
                    return;
                }
            }
        }
    }
}

/// Like [`fetch_with_retry`] but for fetchers that take a [`QuerySignal`].
///
/// On retry, reads a fresh signal from the resource entity and passes it to the fetcher.
async fn fetch_signal_with_retry<T, E, F, Fut>(
    fetcher: &F,
    initial_signal: QuerySignal,
    retry_policy: &RetryPolicy,
    entity: &gpui::WeakEntity<QueryResource<T, E>>,
    cx: &mut gpui::AsyncApp,
) where
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: Fn(QuerySignal) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let mut attempt: u32 = 0;
    let mut signal = initial_signal;

    loop {
        let result = fetcher(signal.clone()).await;

        match result {
            Ok(data) => {
                let now_ms = current_time_ms();
                let e = match entity.upgrade() {
                    Some(e) => e,
                    None => return,
                };
                e.update(cx, |resource, cx| {
                    resource.reset_retry_count();
                    if let Some(guard) = resource.accept_current_request(
                        resource
                            .active_request_id()
                            .unwrap_or(crate::core::RequestId::scoped(0, 0)),
                    ) {
                        resource.complete_success(&guard, data, now_ms);
                    }
                    cx.notify();
                });
                return;
            }
            Err(error) => {
                if retry_policy.should_retry(attempt) {
                    let delay_ms = retry_policy.delay_for_attempt(attempt);
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    e.update(cx, |resource, cx| {
                        resource.increment_retry();
                        cx.notify();
                    });
                    attempt += 1;

                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(delay_ms))
                            .await;
                    }

                    // Get a fresh signal for the next attempt
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    signal = e.read_with(cx, |r, _| {
                        r.signal().cloned().unwrap_or_else(QuerySignal::new)
                    });
                } else {
                    let e = match entity.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    e.update(cx, |resource, cx| {
                        if let Some(guard) = resource.accept_current_request(
                            resource
                                .active_request_id()
                                .unwrap_or(crate::core::RequestId::scoped(0, 0)),
                        ) {
                            resource.complete_failure(&guard, error);
                        }
                        cx.notify();
                    });
                    return;
                }
            }
        }
    }
}

// ── Mutation hooks ──────────────────────────────────────────────────────

/// Hook for executing mutations (create, update, delete operations).
///
/// Creates a [`MutationResource`] entity. Returns the entity for state
/// observation during render. Use the [`mutate`] helper to trigger the
/// mutation from event handlers.
///
/// # Example
///
/// ```ignore
/// struct MyView {
///     create_user: Entity<MutationResource<NewUser, User>>,
/// }
///
/// impl MyView {
///     fn new(cx: &mut Context<Self>) -> Self {
///         let entity = use_mutation(cx);
///         Self { create_user: entity }
///     }
///
///     fn handle_submit(&mut self, name: String, cx: &mut Context<Self>) {
///         mutate(&self.create_user, NewUser { name }, |vars| async move {
///             api::create_user(&vars).await
///         }, cx);
///     }
/// }
/// ```
/// Hook to observe all mutation state across the application for a given
/// `(V, T, E)` type triple.
///
/// Returns a snapshot of all [`MutationResource`] entities of the specified
/// types registered in the global [`QueryClient`]. Returns an empty vec
/// if no mutations exist for this type or if no `QueryClient` is set up.
///
/// # Example
///
/// ```ignore
/// let mutations = use_mutation_state::<NewUser, User, QueryError, _>(cx);
/// for entity in &mutations {
///     let status = entity.read(cx).status();
///     // ...
/// }
/// ```
pub fn use_mutation_state<V, T, E, C>(cx: &mut Context<C>) -> Vec<Entity<MutationResource<V, T, E>>>
where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
    C: 'static,
{
    if cx.has_global::<QueryClient>() {
        cx.read_global::<QueryClient, _>(|client, _| client.all_mutations::<V, T, E>())
    } else {
        Vec::new()
    }
}

/// Hook for executing mutations (create, update, delete operations).
pub fn use_mutation<V, T, E, C>(
    cx: &mut Context<C>,
) -> Entity<MutationResource<V, T, E>>
where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
    C: 'static,
{
    cx.new(|_| MutationResource::new(RetryPolicy::no_retries()))
}

/// Hook for executing mutations with a custom retry policy.
///
/// Like [`use_mutation`] but configures the retry policy on the entity.
pub fn use_mutation_with_options<V, T, E, C>(
    options: &MutationOptions<V, T, E>,
    cx: &mut Context<C>,
) -> Entity<MutationResource<V, T, E>>
where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
    C: 'static,
{
    cx.new(|_| MutationResource::new(options.retry_policy.clone()))
}

/// Trigger a mutation on an existing mutation entity.
///
/// This is the primary way to execute mutations. It:
/// 1. Transitions the entity to Loading with the given variables
/// 2. Spawns an async task calling the mutator
/// 3. On success, completes with the result data
/// 4. On failure, retries according to the entity's retry policy
///
/// # Example
///
/// ```ignore
/// mutate(&entity, variables, |v| async move { Ok(v) }, cx);
/// ```
pub fn mutate<V, T, E, C, F, Fut>(
    entity: &Entity<MutationResource<V, T, E>>,
    variables: V,
    mutator: F,
    cx: &mut Context<C>,
) where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    F: Fn(V) -> Fut + 'static + Clone,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    // Begin the mutation: transition to Loading
    entity.update(cx, |resource, cx| {
        resource.begin(variables.clone(), current_time_ms() as u64);
        cx.notify();
    });

    let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
    let weak = entity.downgrade();
    cx.spawn(async move |_this, cx| {
        run_mutation_loop(&weak, variables, &mutator, &retry_policy, cx).await;
        Ok::<_, ()>(())
    })
    .detach();
}

/// Like [`mutate`] but with lifecycle callbacks.
///
/// Callbacks fire on the final outcome (after all retries exhausted or
/// on first success), not on intermediate retry attempts.
pub fn mutate_with_callbacks<V, T, E, C, F, Fut>(
    entity: &Entity<MutationResource<V, T, E>>,
    variables: V,
    mutator: F,
    callbacks: MutationCallbacks<T, E>,
    cx: &mut Context<C>,
) where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    C: 'static,
    F: Fn(V) -> Fut + 'static + Clone,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    // Begin the mutation
    entity.update(cx, |resource, cx| {
        resource.begin(variables.clone(), current_time_ms() as u64);
        cx.notify();
    });

    let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
    let weak = entity.downgrade();
    cx.spawn(async move |_this, cx| {
        run_mutation_loop_with_callbacks(
            &weak,
            variables,
            &mutator,
            &retry_policy,
            callbacks,
            cx,
        )
        .await;
        Ok::<_, ()>(())
    })
    .detach();
}

// ── Mutation internals ──────────────────────────────────────────────────

/// Core retry loop for mutations. Runs the mutator, handles success/failure,
/// and retries with backoff according to the retry policy.
async fn run_mutation_loop<V, T, E, F, Fut>(
    weak: &gpui::WeakEntity<MutationResource<V, T, E>>,
    variables: V,
    mutator: &F,
    retry_policy: &RetryPolicy,
    cx: &mut gpui::AsyncApp,
) where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: Fn(V) -> Fut + 'static + Clone,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let mut attempt: u32 = 0;

    loop {
        let result = mutator(variables.clone()).await;

        match result {
            Ok(data) => {
                let entity = match weak.upgrade() {
                    Some(e) => e,
                    None => return,
                };
                entity.update(cx, |resource, cx| {
                    resource.complete_success(data);
                    cx.notify();
                });
                return;
            }
            Err(error) => {
                let entity = match weak.upgrade() {
                    Some(e) => e,
                    None => return,
                };

                entity.update(cx, |resource, cx| {
                    resource.complete_failure(error);
                    cx.notify();
                });

                if retry_policy.should_retry(attempt) {
                    let delay_ms = retry_policy.delay_for_attempt(attempt);

                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(delay_ms))
                            .await;
                    }

                    let entity = match weak.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    entity.update(cx, |resource, cx| {
                        resource.retry();
                        cx.notify();
                    });

                    attempt += 1;
                } else {
                    return;
                }
            }
        }
    }
}

/// Like [`run_mutation_loop`] but fires lifecycle callbacks on final outcome.
async fn run_mutation_loop_with_callbacks<V, T, E, F, Fut>(
    weak: &gpui::WeakEntity<MutationResource<V, T, E>>,
    variables: V,
    mutator: &F,
    retry_policy: &RetryPolicy,
    callbacks: MutationCallbacks<T, E>,
    cx: &mut gpui::AsyncApp,
) where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + std::fmt::Debug + 'static,
    F: Fn(V) -> Fut + 'static + Clone,
    Fut: std::future::Future<Output = Result<T, E>> + Send + 'static,
{
    let mut attempt: u32 = 0;

    loop {
        let result = mutator(variables.clone()).await;

        match result {
            Ok(data) => {
                let entity = match weak.upgrade() {
                    Some(e) => e,
                    None => return,
                };
                entity.update(cx, |resource, cx| {
                    resource.complete_success(data);
                    cx.notify();
                });

                // Fire success callback
                if let Some(ref cb) = callbacks.on_success {
                    let entity = match weak.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    entity.read_with(cx, |resource, _| {
                        if let Some(data) = resource.data() {
                            cb(data);
                        }
                    });
                }

                // Fire settled callback
                if let Some(ref cb) = callbacks.on_settled {
                    let entity = match weak.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    entity.read_with(cx, |resource, _| {
                        cb(resource.data(), None);
                    });
                }

                return;
            }
            Err(error) => {
                let entity = match weak.upgrade() {
                    Some(e) => e,
                    None => return,
                };

                entity.update(cx, |resource, cx| {
                    resource.complete_failure(error);
                    cx.notify();
                });

                if retry_policy.should_retry(attempt) {
                    let delay_ms = retry_policy.delay_for_attempt(attempt);

                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(delay_ms))
                            .await;
                    }

                    let entity = match weak.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    entity.update(cx, |resource, cx| {
                        resource.retry();
                        cx.notify();
                    });

                    attempt += 1;
                } else {
                    // No more retries — fire error and settled callbacks
                    if let Some(ref cb) = callbacks.on_error {
                        let entity = match weak.upgrade() {
                            Some(e) => e,
                            None => return,
                        };
                        entity.read_with(cx, |resource, _| {
                            if let Some(error) = resource.error() {
                                cb(error);
                            }
                        });
                    }

                    if let Some(ref cb) = callbacks.on_settled {
                        let entity = match weak.upgrade() {
                            Some(e) => e,
                            None => return,
                        };
                        entity.read_with(cx, |resource, _| {
                            cb(None, resource.error());
                        });
                    }

                    return;
                }
            }
        }
    }
}

/// Returns current time as milliseconds since UNIX epoch.
pub fn current_time_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
