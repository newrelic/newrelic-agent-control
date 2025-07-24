use std::thread::sleep;
use std::time::Duration;

/// Retries the execution of `f` after the `interval` has elapsed, until `max_attempts` is reached.
/// Returns the result of the last successful execution of `f` or the latest error if all attempts fail.
pub fn retry<F, T, E>(max_attempts: usize, interval: Duration, mut f: F) -> Result<T, E>
where
    F: FnMut() -> Result<T, E>,
{
    let mut last_err = None;
    for _ in 0..max_attempts {
        match f() {
            Ok(result) => return Ok(result),
            Err(err) => {
                last_err = Some(err);
                sleep(interval);
            }
        }
    }
    Err(last_err.expect("some error must exist at this point"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_success() {
        let result: Result<&str, &str> = retry(3, Duration::from_millis(10), || Ok("success"));
        assert_eq!(result, Ok("success"));
    }

    #[test]
    fn test_retry_failure() {
        let result: Result<&str, &str> = retry(3, Duration::from_millis(10), || Err("failure"));
        assert_eq!(result, Err("failure"));
    }

    #[test]
    fn test_retry_with_multiple_attempts() {
        let mut attempts = 0;
        let result = retry(3, Duration::from_millis(10), || {
            attempts += 1;
            if attempts < 3 {
                Err("try again")
            } else {
                Ok("finally succeeded")
            }
        });
        assert_eq!(result, Ok("finally succeeded"));
    }
}
