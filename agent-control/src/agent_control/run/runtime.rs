#[cfg(test)]
pub mod tests {
    use std::sync::{Arc, OnceLock};

    /// Returns a static reference to the tokio runtime. The runtime is built the first time this function
    /// is called.
    /// Creating a new runtime in different unit tests can be problematic and lead to flaky tests because
    /// the corresponding thread-level resources get shutdown on drop.
    pub fn tokio_runtime() -> Arc<tokio::runtime::Runtime> {
        static RUNTIME_ONCE: OnceLock<Arc<tokio::runtime::Runtime>> = OnceLock::new();
        RUNTIME_ONCE
            .get_or_init(|| {
                Arc::new(
                    tokio::runtime::Builder::new_multi_thread()
                        .worker_threads(5)
                        .enable_all()
                        .build()
                        .unwrap(),
                )
            })
            .clone()
    }
}
