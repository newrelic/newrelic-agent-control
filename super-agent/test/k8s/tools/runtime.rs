use std::sync::{Arc, OnceLock};

use futures::Future;

/// Returns a static reference to the tokio runtime. The runtime is built the first time this function
/// is called.
pub fn tokio_runtime() -> Arc<tokio::runtime::Runtime> {
    static RUNTIME_ONCE: OnceLock<Arc<tokio::runtime::Runtime>> = OnceLock::new();
    RUNTIME_ONCE
        .get_or_init(|| {
            Arc::new(
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .unwrap(),
            )
        })
        .clone()
}

/// A wrapper to shorten the usage of the runtime's block_on. It is useful because most synchronous
/// tests need to perform some calls to async functions.
pub fn block_on<F: Future>(future: F) -> F::Output {
    tokio_runtime().block_on(future)
}
