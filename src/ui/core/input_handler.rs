use gpui::{Context, Entity, Subscription, Window};
use gpui_component::input::{InputEvent, InputState};

/// Generic input helper wrapping an [`Entity<InputState>`].
///
/// Consolidates the input-subscription boilerplate that is otherwise repeated
/// across feature pages (subscribe → read value → fire view callback). Generic
/// over the owning view `V`, so the same handler works for any GPUI view.
pub struct InputHandler<V> {
    input_state: Entity<InputState>,
    subscription: Option<Subscription>,
    _phantom: std::marker::PhantomData<V>,
}

impl<V: 'static> InputHandler<V> {
    /// Wrap an existing [`InputState`] entity.
    pub fn new(input_state: Entity<InputState>) -> Self {
        Self {
            input_state,
            subscription: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Borrow the underlying input state entity.
    pub fn input_state(&self) -> &Entity<InputState> {
        &self.input_state
    }

    /// Subscribe to input changes with a callback.
    ///
    /// `on_change` is invoked with `(view, text, cx)` whenever the
    /// [`InputState`] emits [`InputEvent::Change`]. The current text is read
    /// from the state for the callback. Replaces any prior subscription.
    pub fn subscribe<F>(&mut self, cx: &mut Context<V>, mut on_change: F) -> &mut Self
    where
        F: FnMut(&mut V, String, &mut Context<V>) + 'static,
    {
        let input_state = self.input_state.clone();
        let input_state_for_read = input_state.clone();
        self.subscription = Some(cx.subscribe(
            &input_state,
            move |view, _input, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let text = input_state_for_read.read(cx).value().to_string();
                    on_change(view, text, cx);
                }
            },
        ));
        self
    }

    /// Set the input placeholder.
    pub fn set_placeholder<T>(&self, placeholder: &str, window: &mut Window, cx: &mut Context<T>) {
        let placeholder = placeholder.to_string();
        self.input_state.update(cx, move |input, cx| {
            input.set_placeholder(&placeholder, window, cx);
        });
    }

    /// Set the input value programmatically.
    pub fn set_value<T>(&self, value: &str, window: &mut Window, cx: &mut Context<T>) {
        let value = value.to_string();
        self.input_state.update(cx, move |input, cx| {
            input.set_value(&value, window, cx);
        });
    }

    /// Read the current input value as an owned [`String`].
    pub fn value<T>(&self, cx: &Context<T>) -> String {
        self.input_state.read(cx).value().to_string()
    }

    /// Clear the input (sets value to the empty string).
    pub fn clear<T>(&self, window: &mut Window, cx: &mut Context<T>) {
        self.set_value("", window, cx);
    }

    /// Move keyboard focus to the wrapped input.
    pub fn focus<T>(&self, window: &mut Window, cx: &mut Context<T>) {
        self.input_state.update(cx, |input, cx| {
            input.focus(window, cx);
        });
    }
}
