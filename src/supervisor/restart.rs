use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct RestartPolicy {
    backoff: BackoffStrategy,
    restart_exit_codes: Vec<i32>
}

impl RestartPolicy{
    pub fn new(backoff: BackoffStrategy, restart_exit_codes: Vec<i32>) -> Self {
        Self {
            backoff,
            restart_exit_codes,
        }
    }

    pub fn should_retry(&mut self, exit_code: i32) -> bool {
        return self.exit_code_triggers_restart(exit_code) && self.backoff.should_backoff()
    }

    pub fn backoff(&mut self) {
        self.backoff.backoff()
    }

    fn exit_code_triggers_restart(&self, exit_code: i32) -> bool {
        let mut retry = false;

        self.restart_exit_codes.iter().for_each(|code| {
            if *code == exit_code {
                retry = true;
            }
        });
        retry
    }
}


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
    fn should_backoff(&mut self) -> bool {
        match self {
            BackoffStrategy::Fixed(ref mut b) => b.should_backoff(),
            BackoffStrategy::Linear(ref mut b) => b.should_backoff(),
            BackoffStrategy::Exponential(ref mut b) => b.should_backoff(),
            BackoffStrategy::None => true,
        }
    }

    fn backoff(&mut self) {
        match self {
            BackoffStrategy::Fixed(ref mut b) => b.backoff(fixed, sleep),
            BackoffStrategy::Linear(ref mut b) => b.backoff(linear, sleep),
            BackoffStrategy::Exponential(ref mut b) => b.backoff(exponential, sleep),
            BackoffStrategy::None => { },
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

    fn should_backoff(&mut self) -> bool {
        let duration = self.last_retry.elapsed();
        if duration > self.last_retry_interval {
            self.tries = 0
        }
        self.tries += 1;

        return self.max_retries == 0 || self.tries <= self.max_retries
    }

    fn backoff<B, S>(&mut self, backoff_func: B, sleep_func: S)
        where
            B: FnOnce(usize, Duration, S),
            S: FnOnce(Duration),
    {
        backoff_func(self.tries, self.initial_delay, sleep_func);
        self.last_retry = Instant::now();
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
            let should_backoff = b.should_backoff();
            assert_eq!(results[n], should_backoff);
            if should_backoff {
                b.backoff(linear, &mut sleep_mock);
            }
        }
        assert_eq!(Duration::from_secs(3), slept);
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
            let should_backoff = b.should_backoff();
            assert_eq!(results[n], should_backoff);
            if should_backoff {
                b.backoff(linear, &mut sleep_mock);
            }
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
            let should_backoff = b.should_backoff();
            assert_eq!(results[n], should_backoff);
            if should_backoff {
                b.backoff(linear, &mut sleep_mock);
            }
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
            let should_backoff = b.should_backoff();
            assert_eq!(results[n], should_backoff);
            if should_backoff {
                b.backoff(fixed, &mut sleep_mock);
            }
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
            let should_backoff = b.should_backoff();
            assert_eq!(results[n], should_backoff);
            if should_backoff {
                b.backoff(exponential, &mut sleep_mock);
            }
        }
        assert_eq!(Duration::from_secs(15), slept)
    }
}
