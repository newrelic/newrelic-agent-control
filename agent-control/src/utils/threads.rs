use std::thread;

pub fn spawn_named_thread<F, T, S>(name: S, f: F) -> thread::JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
    S: ToString,
{
    thread::Builder::new()
        .name(name.to_string())
        .spawn(f)
        .expect("thread config should be valid")
}
