use std::sync::OnceLock;

// TODO: avoid global variable
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME_ONCE: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME_ONCE.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
    })
}
