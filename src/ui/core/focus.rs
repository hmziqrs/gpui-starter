use gpui::{App, Context, FocusHandle, Focusable, Subscription, Window};

/// Manages focus for a view with automatic blur handling.
///
/// Wraps a single [`FocusHandle`] together with an optional blur
/// [`Subscription`] so feature pages can request focus and react to focus loss
/// without each managing their own bookkeeping. Designed to be held as a field
/// on a GPUI view (`Context<V>`).
pub struct FocusManager {
    focus_handle: FocusHandle,
    blur_subscription: Option<Subscription>,
}

impl FocusManager {
    /// Create a new focus manager bound to `cx`'s focus handle pool.
    ///
    /// Generic over the owning view `T` so it can be constructed inside any
    /// `Context<T>` (matching `cx.focus_handle()`).
    pub fn new<T>(cx: &mut Context<T>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            blur_subscription: None,
        }
    }

    /// Borrow the underlying [`FocusHandle`].
    pub fn handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// Subscribe to blur events (when focus is lost).
    ///
    /// The callback receives the view, the window, and the context, mirroring
    /// [`Context::on_blur`]. Calling this replaces any previously registered
    /// blur subscription. Returns `&mut self` for builder-style chaining.
    pub fn on_blur<V, F>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<V>,
        callback: F,
    ) -> &mut Self
    where
        V: 'static,
        F: Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
    {
        let handle = self.focus_handle.clone();
        self.blur_subscription = Some(cx.on_blur(&handle, window, callback));
        self
    }

    /// Request focus for this view's handle.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus_handle, cx);
    }

    /// Whether this handle currently has focus in `window`.
    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }
}

impl Focusable for FocusManager {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
