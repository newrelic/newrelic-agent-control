use std::time::{Duration, Instant};
use std::thread::sleep;

pub enum BackoffStrategy {
    Linear(Linear),
    Exponential(Exponential),
    None,
}

/// Time Duration interval since last retry to consider a service malfunctioning so backoff strategy
/// should keep its sequence, if duration is higher, then the backoff will reset its values to start a new sequence
const LAST_RETRY_INTERVAL:Duration = Duration::new(30, 0);

impl BackoffStrategy {
    pub(crate) fn backoff(&self) -> bool {
        match self {
            BackoffStrategy::Linear(mut b) => b.backoff(),
            BackoffStrategy::Exponential(mut b) => b.backoff(),
            BackoffStrategy::None => true,
        }
    }
}

pub struct Linear {
    last_retry: Instant,
    tries: usize,
    initial_delay: Duration,
    max_retries: usize,
}
impl Linear {
    pub(crate) fn new() -> Self {
        Self { last_retry: Instant::now(), tries: 0, initial_delay: Duration::new(0, 0), max_retries: 0 }
    }

    pub(crate) fn with_initial_delay(mut self, initial_delay: Duration) -> Self {
        self.initial_delay = initial_delay;
        self
    }

    pub(crate) fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub(crate) fn backoff(&mut self) -> bool {
        let duration = self.last_retry.elapsed();
        if duration > LAST_RETRY_INTERVAL {
            self.tries = 0
        }

        if self.tries > self.max_retries {
            false
        }

        sleep(self.tries * self.initial_delay);

        self.tries += 1;
        self.last_retry = Instant::now();

        true
    }
}

pub struct Exponential {
    last_retry: Instant,
    tries: usize,
    initial_delay: string,
    max_retries: usize,
}
impl Exponential {
    pub(crate) fn new() -> Self {
        Self { last_retry: Instant::now(), tries: 0, initial_delay: Duration::new(0, 0), max_retries: 0 }
    }

    pub(crate) fn with_initial_delay(mut self, initial_delay: Duration) -> Self {
        self.initial_delay = initial_delay;
        self
    }

    pub(crate) fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub(crate) fn backoff(&mut self) -> bool {
        let duration = self.last_retry.elapsed();
        if duration > LAST_RETRY_INTERVAL {
            self.tries = 0
        }

        if self.tries > self.max_retries {
            false
        }

        let base: i32 = 2;
        sleep(self.initial_delay * base.pow(self.tries as u32 - 1));

        self.tries += 1;
        self.last_retry = Instant::now();

        true
    }
}
