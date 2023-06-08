use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub enum BackoffStrategy {
    Fixed(Backoff),
    Linear(Backoff),
    Exponential(Backoff),
    None,
}

/// Time Duration interval since last retry to consider a service malfunctioning so backoff strategy
/// should keep its sequence, if duration is higher, then the backoff will reset its values to start a new sequence
const LAST_RETRY_INTERVAL: Duration = Duration::new(30, 0);

impl BackoffStrategy {
    pub(crate) fn backoff(&mut self) -> bool {
        match self {
            BackoffStrategy::Fixed(ref mut b) => b.backoff(fixed, sleep),
            BackoffStrategy::Linear(ref mut b) => b.backoff(linear, sleep),
            BackoffStrategy::Exponential(ref mut b) => b.backoff(exponential, sleep),
            BackoffStrategy::None => true,
        }
    }
}

#[derive(Clone)]
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
            initial_delay: Duration::new(1, 0),
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

    pub(crate) fn with_last_retry_interval(mut self, last_retry_interval: Duration) -> Self {
        self.last_retry_interval = last_retry_interval;
        self
    }

    pub(crate) fn backoff<B, S>(&mut self, backoff_func: B, sleep_func: S) -> bool
    where
        B: FnOnce(usize, Duration, S),
        S: FnOnce(Duration),
    {
        let duration = self.last_retry.elapsed();
        if duration > self.last_retry_interval {
            self.tries = 0
        }
        self.tries += 1;

        if self.max_retries != 0 && self.tries > self.max_retries {
            return false;
        }

        backoff_func(self.tries, self.initial_delay, sleep_func);

        self.last_retry = Instant::now();

        true
    }
}

/// fixed is a function executing a sleep function with a delay incrementing linearly
pub fn fixed<S>(_: usize, initial_delay: Duration, sleep_func: S)
where
    S: FnOnce(Duration),
{
    sleep_func(initial_delay);
}

/// linear is a function executing a sleep function with a delay incrementing linearly
pub fn linear<S>(tries: usize, initial_delay: Duration, sleep_func: S)
where
    S: FnOnce(Duration),
{
    let total_secs_duration = tries as f32 * initial_delay.as_secs_f32();
    sleep_func(Duration::from_secs_f32(total_secs_duration));
}

/// exponential is a function executing a sleep function with a delay incrementing exponentially in base 2
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
    use std::time::Duration;

    #[test]
    fn test_backoff_linear_max_retries_reached() {
        let mut slept = Duration::new(0, 0);
        let mut sleep_mock = |dur: Duration| slept += dur;

        let mut b = Backoff::new().with_max_retries(2);
        let results = vec![true, true, false];

        for n in 0..results.capacity() {
            assert_eq!(results[n], b.backoff(linear, &mut sleep_mock));
        }
        assert_eq!(Duration::from_secs(3), slept)
    }

    #[test]
    fn test_backoff_linear_max_retries_reached_but_interval_reset() {
        let mut slept = Duration::new(0, 0);
        let mut sleep_mock = |dur: Duration| slept += dur;

        let mut b = Backoff::new()
            .with_max_retries(2)
            .with_last_retry_interval(Duration::from_micros(1));
        let results = vec![true, true, true];

        for n in 0..results.capacity() {
            assert_eq!(results[n], b.backoff(linear, &mut sleep_mock));
            //It will be reset every interval causing backoff to always be 1 second
            sleep(Duration::from_micros(2))
        }
        assert_eq!(Duration::from_secs(3), slept)
    }

    #[test]
    fn test_backoff_linear_with_initial_delay() {
        let mut slept = Duration::new(0, 0);
        let mut sleep_mock = |dur: Duration| slept += dur;

        let mut b = Backoff::new().with_initial_delay(Duration::from_secs(6));
        let results = vec![true, true, true];

        for n in 0..results.capacity() {
            assert_eq!(results[n], b.backoff(linear, &mut sleep_mock));
        }
        assert_eq!(Duration::from_secs(36), slept)
    }

    #[test]
    fn test_backoff_fixed() {
        let mut slept = Duration::new(0, 0);
        let mut sleep_mock = |dur: Duration| slept += dur;

        let mut b = Backoff::new();
        let results = vec![true, true, true, true];

        for n in 0..results.capacity() {
            assert_eq!(results[n], b.backoff(fixed, &mut sleep_mock));
        }
        assert_eq!(Duration::from_secs(4), slept)
    }

    #[test]
    fn test_backoff_exponential() {
        let mut slept = Duration::new(0, 0);
        let mut sleep_mock = |dur: Duration| slept += dur;

        let mut b = Backoff::new();
        let results = vec![true, true, true, true];

        for n in 0..results.capacity() {
            assert_eq!(results[n], b.backoff(exponential, &mut sleep_mock));
        }
        assert_eq!(Duration::from_secs(15), slept)
    }
}
