use std::sync::OnceLock;

/// Returns a static reference to a tokio runtime initialized on first usage.
/// It can be used (in code not running already in a tokio-runtime context) to
/// to perform a `tokio_runtime().block_on(<future>)` call and wait for its
/// completion.
/// It uses the default tokio configuration (the same that #[tokio::main]).
// TODO: avoid the need of this global reference
pub fn tokio_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME_ONCE: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME_ONCE.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
    })
}
