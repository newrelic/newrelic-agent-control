use std::time::{Duration, Instant};
use std::thread::sleep;

pub enum BackoffStrategy {
    Linear(Backoff),
    Exponential(Backoff),
    None,
}

/// Time Duration interval since last retry to consider a service malfunctioning so backoff strategy
/// should keep its sequence, if duration is higher, then the backoff will reset its values to start a new sequence
const LAST_RETRY_INTERVAL:Duration = Duration::new(30, 0);

impl BackoffStrategy {
    pub(crate) fn backoff(&self) -> bool {
        match self {
            BackoffStrategy::Linear(mut b) => b.backoff(linear, sleep),
            // BackoffStrategy::Linear(mut b) => b.backoff(, |x| sleep(x)),
            BackoffStrategy::Exponential(mut b) => b.backoff(exponential, sleep),
            BackoffStrategy::None => true,
        }
    }
}

pub struct Backoff {
    last_retry: Instant,
    tries: usize,
    initial_delay: Duration,
    max_retries: usize,
    last_retry_interval: Duration,
}
impl Backoff {
    pub(crate) fn new() -> Self {
        Self {
            last_retry: Instant::now(),
            tries: 0,
            initial_delay: Duration::new(0, 0),
            max_retries: 0,
            last_retry_interval: LAST_RETRY_INTERVAL,
        }
    }

    pub(crate) fn with_initial_delay(mut self, initial_delay: Duration) -> Self {
        self.initial_delay = initial_delay;
        self
    }

    pub(crate) fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub(crate) fn with_last_default_interval(mut self, last_retry_interval: Duration) -> Self {
        self.last_retry_interval = last_retry_interval;
        self
    }

    pub(crate) fn backoff<B, S>(&mut self, backoff_func: B, sleep_func: S) -> bool
        where
        B: FnOnce(usize, Duration, S),
        S: FnOnce(Duration),
    {
        let duration = self.last_retry.elapsed();
        if duration > LAST_RETRY_INTERVAL {
            self.tries = 0
        }

        if self.tries > self.max_retries {
            return false
        }

        backoff_func(self.tries, self.initial_delay, sleep_func);

        self.tries += 1;
        self.last_retry = Instant::now();

        true
    }
}

/// linear is a function
pub fn linear<S>(tries: usize, initial_delay: Duration, sleep_func: S)
    where
        S: FnOnce(Duration),
{
    let total_secs_duration = tries as f32 * initial_delay.as_secs_f32();
    sleep_func(Duration::from_secs_f32(total_secs_duration));
}

/// exponential is a function
pub fn exponential<S>(tries: usize, initial_delay: Duration, sleep_func: S)
    where
        S: FnOnce(Duration),
{
    let base: u32 = 2;
    sleep_func(initial_delay * base.pow(tries as u32 - 1));
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_backoff_linear() {
        let mut b = Backoff::new();

        let mut sleeped = Duration::new(0, 0);
        let sleep_mock = |dur: Duration| {
            sleeped = dur
        };

        b.backoff(linear, sleep_mock);

        assert_eq!(Duration::from_secs(4), sleeped);
    }

    #[test]
    fn test_backoff_exponential() {
        let mut b = Backoff::new();

        let mut sleeped = Duration::new(0, 0);
        let sleep_mock = |dur: Duration| {
            sleeped = dur
        };

        b.backoff(exponential, sleep_mock);

        assert_eq!(Duration::from_secs(4), sleeped);
    }
}
