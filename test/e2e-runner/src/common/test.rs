use std::{error::Error, thread, time::Duration};

use tracing::info;

/// Type alias to represent test results.
pub type TestResult<T> = Result<T, Box<dyn Error>>;

/// Retries the operation `f` as configured.
pub fn retry<F, T>(retries: i64, delay: Duration, err_context: &str, f: F) -> TestResult<T>
where
    F: Fn() -> TestResult<T>,
{
    let mut last_error = String::new();
    for attempt in 1..=retries {
        match f() {
            Ok(result) => return Ok(result),
            Err(err) => {
                last_error = err.to_string();
                info!(%err, "[{attempt}/{retries}] '{err_context}' failed, retrying in {delay:?}");
                thread::sleep(delay);
            }
        }
    }

    Err(last_error.into())
}
