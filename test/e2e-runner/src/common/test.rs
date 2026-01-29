use std::{error::Error, thread, time::Duration};

use tracing::info;

/// Type alias to represent test results.
pub type TestResult<T> = Result<T, Box<dyn Error>>;

/// Retries the operation `f` as configured.
pub fn retry<F, T>(
    retries: i64,
    delay: Duration,
    err_context: impl AsRef<str>,
    f: F,
) -> TestResult<T>
where
    F: Fn() -> TestResult<T>,
{
    let mut last_error = String::new();
    for attempt in 1..=retries {
        match f() {
            Ok(result) => return Ok(result),
            Err(err) => {
                last_error = err.to_string();
                info!(%err, "[{attempt}/{retries}] '{}' failed, retrying in {delay:?}", err_context.as_ref());
                thread::sleep(delay);
            }
        }
    }

    Err(last_error.into())
}

pub fn retry_panic<F>(retries: i64, delay: Duration, err_context: impl AsRef<str>, f: F)
where
    F: Fn() -> TestResult<()>,
{
    retry(retries, delay, err_context.as_ref(), f).unwrap_or_else(|err| {
        panic!(
            "Operation '{}' failed after {} retries: {}",
            err_context.as_ref(),
            retries,
            err
        )
    });
}
