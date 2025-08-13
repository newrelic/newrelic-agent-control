use std::{error::Error, time::Duration};

/// Retries the execution of `f` after the the `interval` has elapsed, until `max_attempts` is reached.
/// # Panics
/// When executing `f` keeps failing after reaching `max_attempts`.
pub fn retry<F>(max_attempts: usize, interval: Duration, mut f: F)
where
    F: FnMut() -> Result<(), Box<dyn Error>>,
{
    let mut last_err = Ok(());
    for _ in 0..max_attempts {
        let Err(err) = f() else {
            return;
        };
        last_err = Err(err);
        std::thread::sleep(interval);
    }
    last_err.unwrap_or_else(|err| panic!("retry failed after {max_attempts} attempts: {err}"))
}

/// DeferredCommand is a struct that allows you to register a cleanup function that is executed on Drop.
pub struct DeferredCommand<F: Fn()> {
    cleanup_fn: F,
}

impl<F: Fn()> DeferredCommand<F> {
    pub fn new(cleanup_fn: F) -> Self {
        Self { cleanup_fn }
    }
}

impl<F: Fn()> Drop for DeferredCommand<F> {
    fn drop(&mut self) {
        (self.cleanup_fn)()
    }
}
