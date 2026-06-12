//! Internal retry loops for mutations.
//!
//! `run_mutation_loop` and `run_mutation_loop_with_callbacks` handle the
//! async retry logic with backoff, cancelled-mutation detection, and
//! lifecycle callback invocation.

use std::sync::Arc;

use crate::core::{MutationResource, RetryPolicy};

use super::super::options::MutationCallbacks;

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
pub(super) async fn run_mutation_loop<V, T, E, F, Fut>(
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
pub(super) async fn run_mutation_loop_with_callbacks<V, T, E, F, Fut>(
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
