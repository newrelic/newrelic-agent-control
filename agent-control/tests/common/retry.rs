use std::{error::Error, time::Duration};

/// Retries the execution of `f` after the the `interval` has elapsed, until `max_attempts` is reached.
/// # Panics
/// When executing `f` keeps failing after reaching `max_attempts`.
pub fn retry<F, T>(max_attempts: usize, interval: Duration, mut f: F) -> T
where
    F: FnMut() -> Result<T, Box<dyn Error>>,
{
    let mut result = None;
    for _ in 0..max_attempts {
        result = Some(f());
        if result.as_ref().is_some_and(|r| r.is_ok()) {
            break;
        }
        std::thread::sleep(interval);
    }

    let Some(final_result) = result else {
        panic!("provided closure never runs");
    };
    final_result.unwrap_or_else(|err| panic!("retry failed after {max_attempts} attempts: {err}"))
}

/// Runs `f` up to `max_attempts` times with `interval` between each call, panicking
/// immediately if it ever returns `Err`.
///
/// The symmetric counterpart to [`retry`]: where `retry` keeps going until success,
/// `retry_never` keeps going until failure — and that failure is itself the test failure.
/// Use this to assert a condition stays true over a stability window rather than checking
/// it only once (which would pass if the condition becomes false a moment later).
pub fn retry_never<F, T>(max_attempts: usize, interval: Duration, mut f: F)
where
    F: FnMut() -> Result<T, Box<dyn Error>>,
{
    for _ in 0..max_attempts {
        f().unwrap_or_else(|err| panic!("retry_never failed: {err}"));
        std::thread::sleep(interval);
    }
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
