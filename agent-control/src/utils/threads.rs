//! Thin wrapper for spawning OS threads with a sanitized name.

use std::thread;

/// Spawns a named thread, stripping NUL bytes from `name` (which the OS thread name forbids).
///
/// Panics if the OS fails to create the thread, matching [`std::thread::spawn`].
pub fn spawn_named_thread<F, T>(name: &str, f: F) -> thread::JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    // Internally, a CString is used. The name must not contain null bytes.
    let clean_name = name.replace('\0', "");
    thread::Builder::new()
        .name(clean_name)
        .spawn(f)
        // Panics if the OS fails to create a thread, as in `std::thread::spawn`.
        .expect("thread config should be valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_named_thread() {
        let handle = spawn_named_thread("test name with\0 null byte", || 1);
        assert_eq!(handle.join().unwrap(), 1);
    }
}
