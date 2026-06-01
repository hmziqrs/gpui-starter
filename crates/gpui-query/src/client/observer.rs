//! Query observer — watches a [`QueryResource`] entity and fires callbacks
//! on status transitions.
//!
//! [`QueryObserver`] is a standalone observer that attaches to a GPUI entity
//! and fires typed callbacks (`on_success`, `on_error`, `on_loading`,
//! `on_settled`) as the query transitions through its lifecycle.
//!
//! # Example
//!
//! ```ignore
//! use gpui_query::client::observer::{QueryObserver, ObserverConfig};
//!
//! let config = ObserverConfig::new()
//!     .on_success(|data: &Vec<User>| { println!("Got {} users", data.len()); })
//!     .on_error(|err: &QueryError| { eprintln!("Error: {}", err.message()); });
//!
//! let mut observer = QueryObserver::new(entity, config);
//! let _subscription = observer.observe(cx);
//! ```

use std::marker::PhantomData;
use std::sync::Arc;

use gpui::{Entity, Subscription};

use crate::core::{QueryResource, QueryStatus};

// ── ObserverConfig ────────────────────────────────────────────────────────

/// Configuration for a query observer, holding optional callbacks.
///
/// All callbacks use `Arc<dyn Fn ... + Send + Sync>` so the config is
/// [`Clone`]-able and can be moved across threads.
///
/// # Builder pattern
///
/// ```
/// use gpui_query::client::observer::ObserverConfig;
///
/// let config: ObserverConfig<String, gpui_query::QueryError> = ObserverConfig::new()
///     .on_success(|data| println!("data: {data}"))
///     .on_loading(|| println!("loading..."));
/// ```
pub struct ObserverConfig<T, E> {
    /// Fired when the resource transitions to `Success`.
    pub on_success: Option<Arc<dyn Fn(&T) + Send + Sync>>,
    /// Fired when the resource transitions to `Failure`.
    pub on_error: Option<Arc<dyn Fn(&E) + Send + Sync>>,
    /// Fired when the resource transitions to a loading state.
    pub on_loading: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Fired on any terminal state (success or failure).
    pub on_settled: Option<Arc<dyn Fn(Option<&T>, Option<&E>) + Send + Sync>>,
    _marker: PhantomData<(T, E)>,
}

impl<T, E> Clone for ObserverConfig<T, E> {
    fn clone(&self) -> Self {
        Self {
            on_success: self.on_success.clone(),
            on_error: self.on_error.clone(),
            on_loading: self.on_loading.clone(),
            on_settled: self.on_settled.clone(),
            _marker: PhantomData,
        }
    }
}

impl<T, E> std::fmt::Debug for ObserverConfig<T, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObserverConfig")
            .field("on_success", &self.on_success.is_some())
            .field("on_error", &self.on_error.is_some())
            .field("on_loading", &self.on_loading.is_some())
            .field("on_settled", &self.on_settled.is_some())
            .finish()
    }
}

impl<T, E> ObserverConfig<T, E> {
    /// Create an empty config (no callbacks).
    pub fn new() -> Self {
        Self {
            on_success: None,
            on_error: None,
            on_loading: None,
            on_settled: None,
            _marker: PhantomData,
        }
    }

    /// Set the success callback.
    pub fn on_success(mut self, f: impl Fn(&T) + Send + Sync + 'static) -> Self {
        self.on_success = Some(Arc::new(f));
        self
    }

    /// Set the error callback.
    pub fn on_error(mut self, f: impl Fn(&E) + Send + Sync + 'static) -> Self {
        self.on_error = Some(Arc::new(f));
        self
    }

    /// Set the loading callback.
    pub fn on_loading(mut self, f: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_loading = Some(Arc::new(f));
        self
    }

    /// Set the settled callback (fires on both success and failure).
    pub fn on_settled(mut self, f: impl Fn(Option<&T>, Option<&E>) + Send + Sync + 'static) -> Self {
        self.on_settled = Some(Arc::new(f));
        self
    }
}

impl<T, E> Default for ObserverConfig<T, E> {
    fn default() -> Self {
        Self::new()
    }
}

// ── QueryObserver ─────────────────────────────────────────────────────────

/// A standalone observer that watches a [`QueryResource`] entity and fires
/// callbacks on status transitions.
///
/// Create with [`new`](QueryObserver::new), configure callbacks via
/// [`ObserverConfig`], then call [`observe`](QueryObserver::observe) to
/// attach to a GPUI context. The returned [`Subscription`] must be kept
/// alive for callbacks to fire.
pub struct QueryObserver<T, E> {
    entity: gpui::WeakEntity<QueryResource<T, E>>,
    config: ObserverConfig<T, E>,
    _last_status: Option<QueryStatus>,
}

impl<T: 'static, E: 'static> QueryObserver<T, E> {
    /// Create a new observer for the given entity with the given config.
    pub fn new(entity: Entity<QueryResource<T, E>>, config: ObserverConfig<T, E>) -> Self {
        Self {
            entity: entity.downgrade(),
            config,
            _last_status: None,
        }
    }

    /// Attach the observer to a context. Returns a [`Subscription`] that
    /// keeps the observer alive.
    ///
    /// The observer fires callbacks based on status transitions of the
    /// underlying resource entity. Call this once during component
    /// initialization and store the returned subscription.
    pub fn observe<W: 'static>(
        &mut self,
        cx: &mut gpui::Context<W>,
    ) -> Subscription {
        let entity = self.entity.clone();
        let config = self.config.clone();

        let upgraded = entity.upgrade().expect("QueryObserver::observe: entity already dropped");

        cx.observe(&upgraded, move |_this, entity, cx| {
            let resource = entity.read(cx);
            let status = resource.status();

            match status {
                QueryStatus::Success => {
                    if let Some(ref cb) = config.on_success {
                        if let Some(data) = resource.data() {
                            cb(data);
                        }
                    }
                }
                QueryStatus::Failure => {
                    if let Some(ref cb) = config.on_error {
                        if let Some(err) = resource.error() {
                            cb(err);
                        }
                    }
                }
                QueryStatus::LoadingEmpty | QueryStatus::LoadingWithData => {
                    if let Some(ref cb) = config.on_loading {
                        cb();
                    }
                }
                _ => {}
            }

            if status == QueryStatus::Success || status == QueryStatus::Failure {
                if let Some(ref cb) = config.on_settled {
                    cb(resource.data(), resource.error());
                }
            }
        })
    }
}
