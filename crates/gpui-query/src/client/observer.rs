//! Query observer for reactive state tracking.
//!
//! **v2 improvements**:
//! - `observe()` returns `Option<Subscription>` instead of panicking on dropped entity
//! - Status deduplication to avoid unnecessary `cx.notify()` calls

use std::cell::Cell;

use gpui::{Context, Entity, Subscription};

use crate::core::{InfiniteQueryResource, MutationResource, MutationStatus, QueryResource, QueryStatus};

/// Configuration for a query observer.
#[derive(Clone, Debug)]
pub struct ObserverConfig {
    /// Only notify when status changes (dedup re-renders).
    pub notify_on_status_change_only: bool,
}

impl Default for ObserverConfig {
    fn default() -> Self {
        Self {
            notify_on_status_change_only: true,
        }
    }
}

/// Observes a query resource and triggers re-renders on state changes.
///
/// In v2, the observer only calls `cx.notify()` when the status actually
/// changes, preventing excessive re-renders from intermediate state updates
/// like retry count increments.
pub struct QueryObserver<T, E> {
    entity: gpui::WeakEntity<QueryResource<T, E>>,
    config: ObserverConfig,
}

impl<T: 'static, E: 'static> QueryObserver<T, E> {
    /// Create a new observer for the given entity.
    pub fn new(entity: &Entity<QueryResource<T, E>>) -> Self {
        Self {
            entity: entity.downgrade(),
            config: ObserverConfig::default(),
        }
    }

    /// Set the observer configuration.
    pub fn with_config(mut self, config: ObserverConfig) -> Self {
        self.config = config;
        self
    }

    /// Start observing the entity. Returns `None` if the entity was already dropped.
    ///
    /// **v2 fix**: Returns `Option<Subscription>` instead of panicking.
    pub fn observe<W: 'static>(&mut self, cx: &mut Context<W>) -> Option<Subscription> {
        let upgraded = self.entity.upgrade()?;
        let notify_on_change = self.config.notify_on_status_change_only;
        let last_status: Cell<Option<QueryStatus>> = Cell::new(None);

        let subscription = cx.observe(&upgraded, move |_, entity, cx| {
            let current_status = entity.read(cx).status();
            if notify_on_change {
                let previous = last_status.get();
                if previous != Some(current_status) {
                    last_status.set(Some(current_status));
                    cx.notify();
                }
            } else {
                cx.notify();
            }
        });

        Some(subscription)
    }
}

/// Observes an infinite query resource and triggers re-renders on state changes.
///
/// Same status-deduplication logic as [`QueryObserver`] but typed for
/// [`InfiniteQueryResource`].
pub struct InfiniteQueryObserver<T, E> {
    entity: gpui::WeakEntity<InfiniteQueryResource<T, E>>,
    config: ObserverConfig,
}

impl<T: 'static, E: 'static> InfiniteQueryObserver<T, E> {
    /// Create a new observer for the given infinite query entity.
    pub fn new(entity: &Entity<InfiniteQueryResource<T, E>>) -> Self {
        Self {
            entity: entity.downgrade(),
            config: ObserverConfig::default(),
        }
    }

    /// Set the observer configuration.
    pub fn with_config(mut self, config: ObserverConfig) -> Self {
        self.config = config;
        self
    }

    /// Start observing the entity. Returns `None` if the entity was already dropped.
    pub fn observe<W: 'static>(&mut self, cx: &mut Context<W>) -> Option<Subscription> {
        let upgraded = self.entity.upgrade()?;
        let notify_on_change = self.config.notify_on_status_change_only;
        let last_status: Cell<Option<QueryStatus>> = Cell::new(None);

        let subscription = cx.observe(&upgraded, move |_, entity, cx| {
            let current_status = entity.read(cx).status();
            if notify_on_change {
                let previous = last_status.get();
                if previous != Some(current_status) {
                    last_status.set(Some(current_status));
                    cx.notify();
                }
            } else {
                cx.notify();
            }
        });

        Some(subscription)
    }
}

/// Observes a mutation resource and triggers re-renders on status changes.
///
/// Same status-deduplication logic as [`QueryObserver`] but typed for
/// [`MutationResource`]. Only calls `cx.notify()` when the mutation's
/// [`MutationStatus`] actually changes, preventing excessive re-renders
/// from intermediate state updates like retry counter increments.
///
/// This is the fix for audit findings #1/#11: the raw `cx.observe` in
/// `use_mutation` unconditionally called `cx.notify()` on every entity
/// mutation, causing 2-3 re-renders per retry attempt. By tracking the
/// last status and only notifying on change, `increment_retry()` and
/// `prepare_retry()` calls (which don't change status -- it stays Loading)
/// no longer trigger re-renders.
pub struct MutationObserver<V, T, E> {
    entity: gpui::WeakEntity<MutationResource<V, T, E>>,
    config: ObserverConfig,
}

impl<V: 'static, T: 'static, E: 'static> MutationObserver<V, T, E> {
    /// Create a new observer for the given mutation entity.
    pub fn new(entity: &Entity<MutationResource<V, T, E>>) -> Self {
        Self {
            entity: entity.downgrade(),
            config: ObserverConfig::default(),
        }
    }

    /// Set the observer configuration.
    pub fn with_config(mut self, config: ObserverConfig) -> Self {
        self.config = config;
        self
    }

    /// Start observing the entity. Returns `None` if the entity was already dropped.
    pub fn observe<W: 'static>(&mut self, cx: &mut Context<W>) -> Option<Subscription> {
        let upgraded = self.entity.upgrade()?;
        let notify_on_change = self.config.notify_on_status_change_only;
        let last_status: Cell<Option<MutationStatus>> = Cell::new(None);

        let subscription = cx.observe(&upgraded, move |_, entity, cx| {
            let current_status = entity.read(cx).status();
            if notify_on_change {
                let previous = last_status.get();
                if previous != Some(current_status) {
                    last_status.set(Some(current_status));
                    cx.notify();
                }
            } else {
                cx.notify();
            }
        });

        Some(subscription)
    }
}
