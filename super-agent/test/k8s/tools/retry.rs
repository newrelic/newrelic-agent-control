use std::{error::Error, time::Duration};

/// Retries the execution of `f` after the the `interval` has elapsed, until `max_attempts` is reached.
/// # Panics
/// When executing `f` keeps failing after reaching `max_attempts`.
pub fn retry<F>(max_attempts: usize, interval: Duration, f: F) -> Result<(), Box<dyn Error>>
where
    F: Fn() -> Result<(), Box<dyn Error>>,
{
    let mut last_err = Ok(());
    for _ in 0..max_attempts {
        if let Err(err) = f() {
            last_err = Err(err)
        } else {
            return Ok(());
        }
        std::thread::sleep(interval);
    }
    last_err
}
