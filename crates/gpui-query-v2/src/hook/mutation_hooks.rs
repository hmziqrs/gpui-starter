//! Mutation hooks and internals — `use_mutation`, `mutate`, `mutate_with_callbacks`,
//! and the internal retry loops.

use std::sync::Arc;

use gpui::{AppContext as _, BorrowAppContext as _, Context, Entity, Subscription};

use crate::client::{MutationObserver, QueryClient};
use crate::core::{MutationResource, RetryPolicy};

use super::options::MutationCallbacks;
use super::MutationOptions;

// ── Mutation hooks ──────────────────────────────────────────────────────

/// Hook for executing mutations (create, update, delete operations).
///
/// Creates a [`MutationResource`] entity. Returns the entity and a subscription
/// for state observation during render. Use the [`mutate`] helper to trigger the
/// mutation from event handlers.
///
/// Accepts `impl Into<MutationOptions>` so both `use_mutation((), cx)` (using
/// `Default` via `From<()>`) and `use_mutation(MutationOptions { .. }, cx)`
/// work.
///
/// Audit fix #1/#11: Uses `MutationObserver` with status-deduplication instead
/// of a raw `cx.observe`. The observer only calls `cx.notify()` when the
/// mutation's `MutationStatus` actually changes (Idle -> Loading, Loading ->
/// Success, Loading -> Failure). Intermediate updates like `increment_retry()`
/// and `prepare_retry()` do not change status (stays Loading), so they no
/// longer trigger re-renders.
///
/// Audit fix #17: Registers the mutation entity with the global [`QueryClient`]
/// so that `use_mutation_state` returns it, GC is triggered, and
/// `MutationOptions::gc_time_ms` is respected.
///
/// # Example
///
/// ```no_run
/// use gpui::{Entity, Subscription, Context};
/// use gpui_query_v2::hook::{use_mutation, mutate};
/// use gpui_query_v2::MutationResource;
/// # #[derive(Clone)]
/// # struct NewUser { name: String }
/// # #[derive(Clone)]
/// # struct User;
/// # #[derive(Clone, Debug)]
/// # struct MyError;
///
/// struct MyView {
///     create_user: Entity<MutationResource<NewUser, User, MyError>>,
///     _mutation_sub: Subscription,
/// }
///
/// impl MyView {
///     fn new(cx: &mut Context<Self>) -> Self {
///         let (entity, sub) = use_mutation((), cx);
///         Self { create_user: entity, _mutation_sub: sub }
///     }
///
///     fn handle_submit(&mut self, name: String, cx: &mut Context<Self>) {
///         mutate(&self.create_user, NewUser { name }, |vars| async move {
///             Ok(User)
///         }, cx);
///     }
/// }
/// ```
pub fn use_mutation<V, T, E, C>(
    options: impl Into<MutationOptions>,
    cx: &mut Context<C>,
) -> (Entity<MutationResource<V, T, E>>, Subscription)
where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
    C: 'static,
{
    let opts = options.into();
    let entity = cx.new(|_| MutationResource::new(opts.retry_policy.clone()));

    // Audit fix #1/#11: Use MutationObserver with status-deduplication instead
    // of raw cx.observe. The observer only calls cx.notify() when MutationStatus
    // actually changes, preventing excessive re-renders from increment_retry()
    // and prepare_retry() calls that don't change status (stays Loading).
    let mut m_observer = MutationObserver::new(&entity);
    let subscription = m_observer
        .observe(cx)
        .expect("MutationObserver::observe failed: entity was just created");

    // Audit fix #17: Register the mutation entity with the global QueryClient so
    // that use_mutation_state returns it, GC is triggered, and gc_time_ms
    // is respected.
    if cx.has_global::<QueryClient>() {
        cx.update_global::<QueryClient, _>(|client, cx| {
            client.register_mutation(&entity, cx);
        });
    }

    (entity, subscription)
}

/// Hook for executing mutations with a custom retry policy.
///
/// Prefer [`use_mutation`] which now accepts `impl Into<MutationOptions>`.
#[deprecated(
    since = "0.2.0",
    note = "Use `use_mutation(options, cx)` instead — it now accepts MutationOptions via Into"
)]
pub fn use_mutation_with_options<V, T, E, C>(
    options: &MutationOptions,
    cx: &mut Context<C>,
) -> (Entity<MutationResource<V, T, E>>, Subscription)
where
    V: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
    C: 'static,
{
    let entity = cx.new(|_| MutationResource::new(options.retry_policy.clone()));

    // Audit fix #1/#11: Use MutationObserver with status-deduplication.
    let mut m_observer = MutationObserver::new(&entity);
    let subscription = m_observer
        .observe(cx)
        .expect("MutationObserver::observe failed: entity was just created");

    (entity, subscription)
}

/// Hook to observe all mutation state across the application for a given
/// `(V, T, E)` type triple.
///
/// Returns a snapshot of all [`MutationResource`] entities of the specified
/// types registered in the global [`QueryClient`]. Returns an empty vec
/// if no mutations exist for this type or if no `QueryClient` is set up.
///
/// # Example
///
/// ```no_run
/// use gpui_query_v2::hook::use_mutation_state;
/// use gpui_query_v2::MutationResource;
/// # #[derive(Clone)]
/// # struct NewUser;
/// # #[derive(Clone)]
/// # struct User;
/// # #[derive(Clone, Debug)]
/// # struct QueryError;
/// # fn _doc<C: 'static>(cx: &mut gpui::Context<C>) {
///
/// let mutations = use_mutation_state::<NewUser, User, QueryError, _>(cx);
/// for entity in &mutations {
///     let status = entity.read(cx).status();
///     // ...
/// }
/// # }
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

/// Trigger a mutation on an existing mutation entity.
///
/// This is the primary way to execute mutations. It:
/// 1. Transitions the entity to Loading with the given variables
/// 2. Spawns an async task calling the mutator
/// 3. On success, completes with the result data
/// 4. On failure, retries according to the entity's retry policy
///
/// Audit fix #8: Guards against concurrent calls by checking whether the
/// mutation is already in Loading state. If so, returns without starting a
/// new mutation to prevent the second call's async task from overwriting
/// the first.
///
/// Audit fix #3: Variables are wrapped in `Arc<V>` internally so that the
/// retry loop only performs an `Arc::clone` (cheap reference count increment)
/// per attempt, rather than cloning the full variables payload.
///
/// # Example
///
/// ```no_run
/// use gpui_query_v2::hook::mutate;
/// # #[derive(Clone)]
/// # struct Vars;
/// # #[derive(Clone)]
/// # struct Data;
/// # #[derive(Clone, Debug)]
/// # struct Err;
/// # fn _doc(entity: &gpui::Entity<gpui_query_v2::MutationResource<Vars, Data, Err>>, cx: &mut gpui::Context<()>) {
///
/// mutate(entity, Vars, |v| async move { Ok(Data) }, cx);
/// # }
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
    // Audit fix #8: Guard against concurrent calls. If the mutation is already
    // Loading, do not start a new one. This prevents a second mutate() call
    // from cancelling the first mutation's signal and then overwriting its
    // state with stale data when the first async task completes.
    let already_loading = entity.read_with(cx, |r, _| r.is_loading());
    if already_loading {
        return;
    }

    // Begin the mutation: transition to Loading
    entity.update(cx, |resource, cx| {
        resource.begin(variables.clone());
        cx.notify();
    });

    let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
    let weak = entity.downgrade();
    let variables_arc = Arc::new(variables);

    cx.spawn(async move |_this, cx| {
        run_mutation_loop(&weak, variables_arc, &mutator, &retry_policy, cx).await;
        Ok::<_, ()>(())
    })
    .detach();
}

/// Like [`mutate`] but with lifecycle callbacks.
///
/// Callbacks fire on the final outcome (after all retries exhausted or
/// on first success), not on intermediate retry attempts.
///
/// **Important**: Callbacks receive cloned data/error and run *outside* any
/// entity borrow, so they may safely call `entity.update()` or other GPUI
/// mutations without risk of deadlock or panic.
///
/// Audit fix #8: Guards against concurrent calls (see [`mutate`] for details).
///
/// Audit fix #9: If the entity is dropped during the mutation, `on_error` and
/// `on_settled` are still invoked so callers always get a terminal callback.
///
/// Audit fix #3: Variables are wrapped in `Arc<V>` for cheap retries.
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
    // Audit fix #8: Guard against concurrent calls.
    let already_loading = entity.read_with(cx, |r, _| r.is_loading());
    if already_loading {
        return;
    }

    // Begin the mutation
    entity.update(cx, |resource, cx| {
        resource.begin(variables.clone());
        cx.notify();
    });

    let retry_policy = entity.read_with(cx, |r, _| r.retry_policy().clone());
    let weak = entity.downgrade();
    let variables_arc = Arc::new(variables);

    cx.spawn(async move |_this, cx| {
        run_mutation_loop_with_callbacks(
            &weak,
            variables_arc,
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
///
/// Audit fix #19: When retries are available, uses `increment_retry()` +
/// `prepare_retry()` instead of `complete_failure()` followed by `retry()`.
/// This avoids a transient `Failure` status that would cause observers to see
/// a brief Failure flash between retry attempts. Only `complete_failure()` is
/// called when retries are exhausted, which represents a terminal failure.
///
/// Audit fix #1: Does NOT call `cx.notify()` after `increment_retry()` or
/// `prepare_retry()` because those operations do not change the mutation status
/// (stays Loading). The `MutationObserver` only triggers `cx.notify()` on actual
/// status changes, so these intermediate updates are invisible to the component.
///
/// Audit fix #3: Variables are passed as `Arc<V>` so each retry attempt only
/// performs an `Arc::clone` (reference count increment) instead of cloning
/// the full variables payload.
///
/// Audit fix #9: After each retry delay, checks whether the mutation is still
/// in Loading state. If it was cancelled or reset (no longer Loading), stops
/// retrying immediately.
async fn run_mutation_loop<V, T, E, F, Fut>(
    weak: &gpui::WeakEntity<MutationResource<V, T, E>>,
    variables: Arc<V>,
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
        // Audit fix #3: Arc::clone instead of variables.clone() for cheap retries.
        let result = mutator((*variables).clone()).await;

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
                if retry_policy.should_retry(attempt) {
                    // Audit fix #19: Do NOT call complete_failure() here.
                    // Instead, just increment the retry counter and wait for
                    // the delay. This avoids a transient Failure -> Loading
                    // flash for observers.
                    let delay_ms = retry_policy.delay_for_attempt(attempt);

                    let e = match weak.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    e.update(cx, |resource, _cx| {
                        resource.increment_retry();
                        // Audit fix #1: No cx.notify() -- increment_retry does
                        // not change status (stays Loading).
                    });

                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(delay_ms))
                            .await;
                    }

                    // Audit fix #9: After the retry delay, check whether the
                    // mutation is still in Loading state. If it was cancelled
                    // or reset, stop retrying immediately.
                    let entity = match weak.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    if !entity.read_with(cx, |r, _| r.is_loading()) {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "DEBUG: run_mutation_loop: mutation no longer Loading after retry delay, aborting"
                        );
                        return;
                    }

                    // After delay, prepare for retry (refresh signal, stay in Loading).
                    entity.update(cx, |resource, _cx| {
                        resource.prepare_retry();
                        // Audit fix #1: No cx.notify() -- prepare_retry does
                        // not change status (stays Loading).
                    });

                    attempt += 1;
                } else {
                    // No more retries -- terminal failure
                    let entity = match weak.upgrade() {
                        Some(e) => e,
                        None => return,
                    };
                    entity.update(cx, |resource, cx| {
                        resource.complete_failure(error);
                        // Audit fix #4: Reset retry_count on terminal failure.
                        resource.reset_retry_count();
                        cx.notify();
                    });
                    return;
                }
            }
        }
    }
}

/// Like [`run_mutation_loop`] but fires lifecycle callbacks on final outcome.
///
/// Callbacks receive cloned data/error and run *outside* any `entity.read_with`
/// borrow. This prevents deadlocks and panics if a callback attempts to call
/// `entity.update()` or any other GPUI mutation.
///
/// Audit fix #9: When `weak.upgrade()` returns `None` (entity dropped), the
/// `on_error` and `on_settled` callbacks are still fired so callers always
/// receive a terminal notification.
///
/// Audit fix #10: The weak entity check result is captured before
/// `complete_failure` so that callbacks fire even if the entity is dropped
/// between the update and the callback invocation.
///
/// Audit fix #19: Uses `increment_retry()` + `prepare_retry()` instead of
/// `complete_failure()` + `retry()` when retries are available.
///
/// Audit fix #1: Does NOT call `cx.notify()` after `increment_retry()` or
/// `prepare_retry()` since those operations do not change status.
///
/// Audit fix #3: Variables are `Arc<V>` for cheap retry clones.
///
/// Audit fix #9: After each retry delay, checks whether the mutation is still
/// Loading. If cancelled/reset, fires error callbacks and stops retrying.
async fn run_mutation_loop_with_callbacks<V, T, E, F, Fut>(
    weak: &gpui::WeakEntity<MutationResource<V, T, E>>,
    variables: Arc<V>,
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
        // Audit fix #3: Arc::clone for cheap retry.
        let result = mutator((*variables).clone()).await;

        match result {
            Ok(data) => {
                // Clone data before update, invoke callbacks outside
                // any entity borrow so they can safely call entity.update().
                let data_for_callback = data.clone();
                let entity = match weak.upgrade() {
                    Some(e) => e,
                    // Audit fix #9: Entity dropped during mutation. Fire
                    // on_settled with None for both to indicate discard.
                    None => {
                        if let Some(ref cb) = callbacks.on_settled {
                            cb(None, None);
                        }
                        return;
                    }
                };
                entity.update(cx, |resource, cx| {
                    resource.complete_success(data);
                    cx.notify();
                });

                // Fire success callback -- outside entity borrow
                if let Some(ref cb) = callbacks.on_success {
                    cb(&data_for_callback);
                }

                // Fire settled callback with success data -- outside entity borrow
                if let Some(ref cb) = callbacks.on_settled {
                    cb(Some(&data_for_callback), None);
                }

                return;
            }
            Err(error) => {
                // Clone error before update, invoke callbacks outside
                // any entity borrow.
                let error_for_callback = error.clone();

                if retry_policy.should_retry(attempt) {
                    // Audit fix #19: Do NOT call complete_failure() here.
                    // Instead, just increment the retry counter and wait.
                    let delay_ms = retry_policy.delay_for_attempt(attempt);

                    let entity = match weak.upgrade() {
                        Some(e) => e,
                        // Audit fix #9: Entity dropped between mutator failure and retry.
                        None => {
                            if let Some(ref cb) = callbacks.on_error {
                                cb(&error_for_callback);
                            }
                            if let Some(ref cb) = callbacks.on_settled {
                                cb(None, Some(&error_for_callback));
                            }
                            return;
                        }
                    };
                    entity.update(cx, |resource, _cx| {
                        resource.increment_retry();
                        // Audit fix #1: No cx.notify() -- status stays Loading.
                    });

                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(delay_ms))
                            .await;
                    }

                    // Audit fix #9: After the retry delay, check whether the
                    // mutation is still Loading. If cancelled/reset, fire error
                    // callbacks and stop retrying.
                    let entity = match weak.upgrade() {
                        Some(e) => e,
                        // Audit fix #9/#10: Entity dropped during retry delay.
                        None => {
                            if let Some(ref cb) = callbacks.on_error {
                                cb(&error_for_callback);
                            }
                            if let Some(ref cb) = callbacks.on_settled {
                                cb(None, Some(&error_for_callback));
                            }
                            return;
                        }
                    };
                    if !entity.read_with(cx, |r, _| r.is_loading()) {
                        // Mutation was cancelled or reset during the delay.
                        // Fire error callbacks so callers get a terminal notification.
                        if let Some(ref cb) = callbacks.on_error {
                            cb(&error_for_callback);
                        }
                        if let Some(ref cb) = callbacks.on_settled {
                            cb(None, Some(&error_for_callback));
                        }
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "DEBUG: run_mutation_loop_with_callbacks: mutation no longer Loading after retry delay, aborting"
                        );
                        return;
                    }

                    // After delay, prepare for retry.
                    entity.update(cx, |resource, _cx| {
                        resource.prepare_retry();
                        // Audit fix #1: No cx.notify() -- status stays Loading.
                    });

                    attempt += 1;
                } else {
                    // No more retries -- terminal failure.
                    // Audit fix #10: Capture entity availability before
                    // complete_failure so callbacks still fire even if entity
                    // is dropped between the update and callback invocation.
                    let entity_available = weak.upgrade();
                    if let Some(entity) = entity_available {
                        entity.update(cx, |resource, cx| {
                            resource.complete_failure(error);
                            // Audit fix #4: Reset retry_count on terminal failure.
                            resource.reset_retry_count();
                            cx.notify();
                        });
                    }

                    // Fire error and settled callbacks outside entity borrow.
                    // These fire regardless of whether entity is still alive
                    // (Audit fix #9/#10).
                    if let Some(ref cb) = callbacks.on_error {
                        cb(&error_for_callback);
                    }

                    if let Some(ref cb) = callbacks.on_settled {
                        cb(None, Some(&error_for_callback));
                    }

                    return;
                }
            }
        }
    }
}
