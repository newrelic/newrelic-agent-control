/// This module contains test utils, is conditionally compiled when #[cfg(tests)].
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
        match f() { Err(err) => {
            last_err = Err(err)
        } _ => {
            return;
        }}
        std::thread::sleep(interval);
    }
    last_err.unwrap_or_else(|err| panic!("retry failed after {max_attempts} attempts: {err}"))
}
