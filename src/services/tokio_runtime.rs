use std::sync::Arc;

use gpui::{App, Global};

/// Dedicated tokio runtime for I/O-bound work (HTTP, etc.).
///
/// GPUI's executor uses its own scheduler (GCD on macOS) and is **not** a tokio
/// runtime. Any code that depends on tokio — `reqwest`, `tokio::net`, `tokio::time`
/// — must run inside this runtime via [`Self::spawn`].
pub struct TokioRuntime {
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub http_client: reqwest::Client,
}

impl TokioRuntime {
    pub fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("gpui-io")
            .build()
            .expect("failed to create tokio runtime");
        tracing::info!(target: "gpui_starter::tokio_runtime", "tokio runtime created");

        // Build the reqwest client inside the runtime context so the connector
        // can resolve DNS via tokio.
        let _guard = runtime.enter();
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("failed to build reqwest client");

        Self {
            runtime: Arc::new(runtime),
            http_client,
        }
    }

    /// Spawn an async task on the tokio runtime.
    pub fn spawn<F>(&self, f: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.runtime.spawn(f)
    }
}

impl Default for TokioRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// GPUI Global that holds the shared tokio runtime.
pub struct TokioRuntimeGlobal(pub TokioRuntime);

impl Global for TokioRuntimeGlobal {}

/// Borrow the shared tokio runtime handle, if a [`TokioRuntimeGlobal`] has been
/// installed. Returns `None` when the runtime is absent (callers should
/// degrade gracefully — e.g. log + return an early result).
pub fn handle(cx: &App) -> Option<Arc<tokio::runtime::Runtime>> {
    cx.try_global::<TokioRuntimeGlobal>()
        .map(|g| g.0.runtime.clone())
}

/// Spawn `future` on the shared tokio runtime, returning the `JoinHandle`.
///
/// Returns `None` when no runtime is installed (graceful degrade). Prefer this
/// over `TokioRuntimeGlobal::0.runtime.spawn(...)` at call sites that already
/// hold an `&App` reference.
pub fn spawn<F>(cx: &App, future: F) -> Option<tokio::task::JoinHandle<F::Output>>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    handle(cx).map(|runtime| runtime.spawn(future))
}
