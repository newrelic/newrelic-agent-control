pub enum BackoffStrategy {
    Linear(Linear),
    Exponential(Exponential),
    Throttle(Throttle),
    None,
}

impl BackoffStrategy {
    pub fn backoff(&self) -> bool {
        match self {
            BackoffStrategy::Linear(b) => b.backoff(),
            BackoffStrategy::Exponential(b) => b.backoff(),
            BackoffStrategy::Throttle(b) => b.backoff(),
            BackoffStrategy::None => true,
        }
    }
}


pub struct Linear {}
impl Linear {
    pub fn backoff(self) -> bool {
        true
    }
}

pub struct Exponential {}
impl Exponential {
    fn backoff() -> bool {
        true
    }
}

pub struct Throttle {}
impl Throttle {
    fn backoff() -> bool {
        true
    }
}

