#[cfg(test)]
pub mod test {
    use std::sync::Arc;

    /// When mock objects are cloned, their expectations cannot be carried over automatically. This
    /// means we need to nest multiple levels of mocked clone to set expectations on the correct
    /// instance. To avoid such boilerplate, we use an Arc<T> inner field to wrap an instance of T.
    /// When cloned, only its reference count is incremented and not the whole mock object.
    pub struct MockWrapper<T> {
        inner: Arc<T>,
    }

    impl<T> Clone for MockWrapper<T> {
        fn clone(&self) -> Self {
            MockWrapper {
                inner: self.inner.clone(),
            }
        }
    }

    impl<T> MockWrapper<T> {
        pub fn new(inner: T) -> MockWrapper<T> {
            MockWrapper {
                inner: Arc::new(inner),
            }
        }

        pub fn get(&self) -> &T {
            self.inner.as_ref()
        }
    }
}
