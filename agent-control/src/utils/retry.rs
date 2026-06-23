use std::num::NonZeroUsize;
use std::thread::sleep;
use std::time::{Duration, SystemTime};

/// Retries the execution of `f` after the `interval` has elapsed, until `max_attempts` is reached.
/// Returns the result of the last successful execution of `f` or the latest error if all attempts fail.
/// If `max_attempts` is zero, it will attempt to execute `f` once.
pub fn retry<F, T, E>(max_attempts: usize, interval: Duration, mut f: F) -> Result<T, E>
where
    F: FnMut() -> Result<T, E>,
{
    // Ensure at least one attempt is made to avoid panic on zero attempts
    let mut sanitized_attempts = max_attempts;
    if max_attempts == 0 {
        sanitized_attempts = 1;
    }

    let mut last_err = None;
    for _ in 0..sanitized_attempts {
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

/// Configuration for retrying an operation that may fail with transient errors.
///
/// When an attempt fails, we wait before retrying with exponential delay —
/// `base_delay`, then `2× base_delay`, then `4×`, etc. — capped at `max_delay`.
///
/// `jitter = true` randomizes each wait somewhere between zero and the computed value.
#[derive(Debug, Clone, PartialEq)]
pub struct BackoffPolicy {
    pub max_attempts: NonZeroUsize,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub jitter: bool,
}

impl BackoffPolicy {
    /// Returns the wait time before the `attempt`-th retry, ignoring jitter.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }

        // 2^(attempt - 1), guarded against unsigned underflow (saturating_sub) and
        // u32 overflow (checked_pow → u32::MAX). The max_delay cap below clamps the
        // resulting Duration regardless.
        let multiplier = 2u32
            .checked_pow(attempt.saturating_sub(1))
            .unwrap_or(u32::MAX);
        let candidate = self
            .base_delay
            .checked_mul(multiplier)
            .unwrap_or(self.max_delay);
        candidate.min(self.max_delay)
    }

    /// Returns the wait time before the `attempt`-th retry, applying [full_jitter] when
    /// `self.jitter` is set. This is the agnostic building block for any backoff schedule —
    /// callers that sleep between attempts and callers that store a "next attempt at" instant
    /// (cooldown windows across polls) share the exact same exponential-capped-plus-jitter math.
    pub fn delay_with_jitter(&self, attempt: u32) -> Duration {
        let delay = self.delay_for_attempt(attempt);
        if self.jitter {
            full_jitter(delay)
        } else {
            delay
        }
    }
}

/// Calls `f`, retrying with exponential backoff (and optional jitter) when it fails.
pub fn retry_with_backoff<F, T, E>(policy: &BackoffPolicy, mut f: F) -> Result<T, E>
where
    F: FnMut() -> Result<T, E>,
{
    let attempts = policy.max_attempts.get();
    let mut last_err = None;
    for attempt in 1..=attempts {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                last_err = Some(e);
                // Don't sleep after the final attempt — there's nothing left to wait for.
                if attempt < attempts {
                    sleep(policy.delay_with_jitter(attempt as u32));
                }
            }
        }
    }
    Err(last_err.expect("at least one attempt must have produced an error"))
}

/// Returns a random duration somewhere in `[0, d]`.
///
/// Each call to produces a different value so retries on different machines
/// (and different attempts on the same machine) don't all happen at the same instant.
/// current system time in nanoseconds is always advancing, and it differs across machine.
pub fn full_jitter(d: Duration) -> Duration {
    let cap_nanos = d.as_nanos() as u64;
    if cap_nanos == 0 {
        return Duration::ZERO;
    }
    let now_nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0);
    // `+ 1` so the upper bound `d` is reachable (we want the inclusive range `[0, d]`).
    Duration::from_nanos(now_nanos % (cap_nanos + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::time::Instant;

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

    #[test]
    fn delay_schedule_exponential_capped() {
        let p = BackoffPolicy {
            max_attempts: NonZeroUsize::new(5).unwrap(),
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(4),
            jitter: false,
        };
        assert_eq!(p.delay_for_attempt(0), Duration::ZERO);
        assert_eq!(p.delay_for_attempt(1), Duration::from_secs(1));
        assert_eq!(p.delay_for_attempt(2), Duration::from_secs(2));
        assert_eq!(p.delay_for_attempt(3), Duration::from_secs(4));
        // Capped at max_delay.
        assert_eq!(p.delay_for_attempt(4), Duration::from_secs(4));
        assert_eq!(p.delay_for_attempt(10), Duration::from_secs(4));
    }

    #[test]
    fn retry_with_backoff_succeeds_after_transient_failures() {
        let attempts = Cell::new(0u32);
        let policy = BackoffPolicy {
            max_attempts: NonZeroUsize::new(4).unwrap(),
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
            jitter: false,
        };
        let result: Result<&str, &str> = retry_with_backoff(&policy, || {
            attempts.set(attempts.get() + 1);
            if attempts.get() < 3 {
                Err("nope")
            } else {
                Ok("ok")
            }
        });
        assert_eq!(result, Ok("ok"));
        assert_eq!(attempts.get(), 3);
    }

    #[test]
    fn retry_with_backoff_gives_up_after_max_attempts() {
        let attempts = Cell::new(0u32);
        let policy = BackoffPolicy {
            max_attempts: NonZeroUsize::new(3).unwrap(),
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
            jitter: false,
        };
        let result: Result<(), &str> = retry_with_backoff(&policy, || {
            attempts.set(attempts.get() + 1);
            Err("permanent")
        });
        assert_eq!(result, Err("permanent"));
        assert_eq!(attempts.get(), 3);
    }

    #[test]
    fn retry_with_backoff_no_sleep_after_final_attempt() {
        // Three attempts with 50ms base — without the "skip sleep on last attempt" guard,
        // a permanently-failing call would sleep ~150ms. With it, ~50+100=150ms is the upper
        // bound for the 1st+2nd inter-attempt sleeps; we just assert it's well under 4× base.
        let policy = BackoffPolicy {
            max_attempts: NonZeroUsize::new(3).unwrap(),
            base_delay: Duration::from_millis(20),
            max_delay: Duration::from_millis(40),
            jitter: false,
        };
        let start = Instant::now();
        let _: Result<(), &str> = retry_with_backoff(&policy, || Err("e"));
        let elapsed = start.elapsed();
        // 20ms after attempt 1 + 40ms after attempt 2 = 60ms; nothing after attempt 3.
        assert!(
            elapsed < Duration::from_millis(100),
            "elapsed {:?} suggests we slept after the final attempt",
            elapsed
        );
    }

    #[test]
    fn jitter_stays_within_bound() {
        let d = Duration::from_millis(100);
        for _ in 0..200 {
            let j = full_jitter(d);
            assert!(j <= d, "{:?} exceeded {:?}", j, d);
        }
    }

    #[test]
    fn jitter_zero_returns_zero() {
        assert_eq!(full_jitter(Duration::ZERO), Duration::ZERO);
    }
}
