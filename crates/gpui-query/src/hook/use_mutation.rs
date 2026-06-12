use gpui::{AppContext as _, Context, Entity};

use crate::client::QueryClient;
use crate::core::{MutationResource, RetryPolicy};

use super::helpers::current_time_ms;
use super::options::{MutationCallbacks, MutationOptions};

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
pub fn use_mutation<V, T, E, C>(cx: &mut Context<C>) -> Entity<MutationResource<V, T, E>>
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
        run_mutation_loop_with_callbacks(&weak, variables, &mutator, &retry_policy, callbacks, cx)
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
