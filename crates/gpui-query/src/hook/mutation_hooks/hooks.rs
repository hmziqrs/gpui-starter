//! Public mutation hooks: `use_mutation`, `mutate`, `mutate_with_callbacks`,
//! and `use_mutation_state`.

use std::sync::Arc;

use gpui::{AppContext as _, BorrowAppContext as _, Context, Entity, Subscription};

use crate::client::{MutationObserver, QueryClient};
use crate::core::MutationResource;

use super::internals::{run_mutation_loop, run_mutation_loop_with_callbacks};
use super::super::options::MutationCallbacks;
use super::super::MutationOptions;

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
/// use gpui_query::hook::{use_mutation, mutate};
/// use gpui_query::MutationResource;
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
/// use gpui_query::hook::use_mutation_state;
/// use gpui_query::MutationResource;
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
/// use gpui_query::hook::mutate;
/// # #[derive(Clone)]
/// # struct Vars;
/// # #[derive(Clone)]
/// # struct Data;
/// # #[derive(Clone, Debug)]
/// # struct Err;
/// # fn _doc(entity: &gpui::Entity<gpui_query::MutationResource<Vars, Data, Err>>, cx: &mut gpui::Context<()>) {
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
