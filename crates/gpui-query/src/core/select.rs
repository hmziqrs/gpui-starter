use std::sync::Arc;

/// A select transform that maps cached data of type T to output type U.
/// Stored as Arc<dyn Fn(&T) -> U> to be Clone + Send + Sync.
pub struct SelectTransform<T, U> {
    transform: Arc<dyn Fn(&T) -> U + Send + Sync>,
    _marker: std::marker::PhantomData<(T, U)>,
}

impl<T, U> Clone for SelectTransform<T, U> {
    fn clone(&self) -> Self {
        Self {
            transform: Arc::clone(&self.transform),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T, U> std::fmt::Debug for SelectTransform<T, U> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectTransform").finish()
    }
}

impl<T, U> SelectTransform<T, U> {
    pub fn new(transform: impl Fn(&T) -> U + Send + Sync + 'static) -> Self {
        Self {
            transform: Arc::new(transform),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn apply(&self, data: &T) -> U {
        (self.transform)(data)
    }
}

/// A mapped view over a QueryResource that applies a select transform.
/// Provides the same read interface but with transformed data type.
#[derive(Clone, Debug)]
pub struct MappedQueryResource<T, U, E> {
    source_data: Option<T>,
    transform: SelectTransform<T, U>,
    _error_marker: std::marker::PhantomData<E>,
}

impl<T, U, E> MappedQueryResource<T, U, E> {
    pub fn new(source_data: Option<T>, transform: SelectTransform<T, U>) -> Self {
        Self {
            source_data,
            transform,
            _error_marker: std::marker::PhantomData,
        }
    }

    pub fn data(&self) -> Option<U> {
        self.source_data.as_ref().map(|d| self.transform.apply(d))
    }

    pub fn has_data(&self) -> bool {
        self.source_data.is_some()
    }
}
