use crate::utils::retry::BackoffPolicy;
use crate::utils::time::{Clock, SystemClock};
use std::sync::Mutex;
use std::time::Instant;

/// The gate's verdict for a given key: attempt the operation now, or suppress it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    /// No active cooldown — the caller should attempt the operation.
    Proceed,
    /// Suppressed: the backoff cooldown window after the last failure has not elapsed yet.
    InCooldown { consecutive_failures: u32 },
    /// Suppressed: `max_attempts` consecutive failures reached; stays suppressed until the key
    /// changes (or [`BackoffGate::reset`] is called).
    CapReached { consecutive_failures: u32 },
}

#[derive(Debug)]
struct GateState<K> {
    key: Option<K>,
    consecutive_failures: u32,
    next_attempt_at: Option<Instant>,
}

// Manual impl (rather than `#[derive(Default)]`) so `K` need not be `Default`.
impl<K> Default for GateState<K> {
    fn default() -> Self {
        Self {
            key: None,
            consecutive_failures: 0,
            next_attempt_at: None,
        }
    }
}

/// A thread-safe gate that throttles repeated attempts at a fallible operation keyed by `K`.
///
/// The operation is identified by a key (e.g. a target version). When an attempt for a key
/// fails, [`BackoffGate::record_failure`] schedules the next permitted attempt using the
/// exponential-backoff-plus-jitter schedule of the configured [`BackoffPolicy`]. Until that
/// instant passes, [`BackoffGate::check`] returns [`GateDecision::InCooldown`]. After
/// `policy.max_attempts` consecutive failures the gate returns [`GateDecision::CapReached`]
/// and stays there until the key changes (a new key implicitly resets the gate) or
/// [`BackoffGate::reset`] is called.
pub struct BackoffGate<K, C = SystemClock> {
    policy: BackoffPolicy,
    clock: C,
    state: Mutex<GateState<K>>,
}

impl<K, C> BackoffGate<K, C>
where
    K: PartialEq + Clone,
    C: Clock,
{
    pub fn new(policy: BackoffPolicy, clock: C) -> Self {
        Self {
            policy,
            clock,
            state: Mutex::new(GateState::default()),
        }
    }

    /// Inspects the gate for `key`, returning whether the caller should proceed or suppress.
    ///
    /// If `key` differs from the last-seen key the gate resets first, so switching targets
    /// always permits an immediate attempt.
    pub fn check(&self, key: &K) -> GateDecision {
        let mut state = self.state.lock().expect("backoff-gate lock poisoned");
        Self::reset_if_key_changed(&mut state, key);

        if (state.consecutive_failures as usize) >= self.policy.max_attempts {
            return GateDecision::CapReached {
                consecutive_failures: state.consecutive_failures,
            };
        }

        if let Some(t) = state.next_attempt_at
            && self.clock.now() < t
        {
            return GateDecision::InCooldown {
                consecutive_failures: state.consecutive_failures,
            };
        }

        GateDecision::Proceed
    }

    /// Records a failed attempt for `key`, incrementing the consecutive-failure count and
    /// scheduling the next permitted attempt per the backoff schedule.
    pub fn record_failure(&self, key: &K) {
        let mut state = self.state.lock().expect("backoff-gate lock poisoned");
        // The key may have changed between `check` and now if another caller ran; only count
        // failures against the key we were actually told failed.
        Self::reset_if_key_changed(&mut state, key);
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        let delay = self.policy.delay_with_jitter(state.consecutive_failures);
        state.next_attempt_at = Some(self.clock.now() + delay);
    }

    /// Clears all failure state (e.g. after a successful attempt).
    pub fn reset(&self) {
        *self.state.lock().expect("backoff-gate lock poisoned") = GateState::default();
    }

    fn reset_if_key_changed(state: &mut GateState<K>, key: &K) {
        if state.key.as_ref() != Some(key) {
            *state = GateState {
                key: Some(key.clone()),
                consecutive_failures: 0,
                next_attempt_at: None,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    /// Test clock backed by `Arc<Mutex<Instant>>` so a test can advance time deterministically
    /// while the gate holds the clock by value.
    #[derive(Clone)]
    struct FakeClock(Arc<Mutex<Instant>>);

    impl FakeClock {
        fn new(initial: Instant) -> Self {
            Self(Arc::new(Mutex::new(initial)))
        }
        fn advance(&self, by: Duration) {
            *self.0.lock().unwrap() += by;
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            *self.0.lock().unwrap()
        }
    }

    fn policy(base: Duration, max: Duration, max_attempts: usize) -> BackoffPolicy {
        BackoffPolicy {
            max_attempts,
            base_delay: base,
            max_delay: max,
            jitter: false,
        }
    }

    fn gate(base: Duration, max: Duration, max_attempts: usize) -> (BackoffGate<&'static str, FakeClock>, FakeClock) {
        let clock = FakeClock::new(Instant::now());
        let gate = BackoffGate::new(policy(base, max, max_attempts), clock.clone());
        (gate, clock)
    }

    #[test]
    fn proceeds_when_no_failures() {
        let (gate, _clock) = gate(Duration::from_secs(1), Duration::from_secs(10), 5);
        assert_eq!(gate.check(&"v1"), GateDecision::Proceed);
    }

    #[test]
    fn suppresses_within_cooldown_window_then_re_attempts() {
        let (gate, clock) = gate(Duration::from_secs(30), Duration::from_secs(600), 5);

        assert_eq!(gate.check(&"v1"), GateDecision::Proceed);
        gate.record_failure(&"v1");

        assert_eq!(
            gate.check(&"v1"),
            GateDecision::InCooldown {
                consecutive_failures: 1
            }
        );

        // base_delay is 30s with no jitter; advancing past it re-opens the gate.
        clock.advance(Duration::from_secs(31));
        assert_eq!(gate.check(&"v1"), GateDecision::Proceed);
    }

    #[test]
    fn cap_reached_suppresses_indefinitely() {
        let (gate, clock) = gate(Duration::from_millis(1), Duration::from_millis(1), 2);

        gate.record_failure(&"v1");
        clock.advance(Duration::from_secs(1));
        gate.record_failure(&"v1");
        clock.advance(Duration::from_secs(1));

        for _ in 0..5 {
            assert_eq!(
                gate.check(&"v1"),
                GateDecision::CapReached {
                    consecutive_failures: 2
                }
            );
        }
    }

    #[test]
    fn key_change_resets_and_allows_immediate_attempt() {
        let (gate, _clock) = gate(Duration::from_secs(60), Duration::from_secs(600), 5);

        gate.record_failure(&"v1");
        assert_eq!(
            gate.check(&"v1"),
            GateDecision::InCooldown {
                consecutive_failures: 1
            }
        );
        // A different key clears the cooldown without advancing the clock.
        assert_eq!(gate.check(&"v2"), GateDecision::Proceed);
    }

    #[test]
    fn reset_clears_failure_state() {
        let (gate, _clock) = gate(Duration::from_secs(60), Duration::from_secs(600), 5);

        gate.record_failure(&"v1");
        gate.reset();
        assert_eq!(gate.check(&"v1"), GateDecision::Proceed);
    }

    #[test]
    fn exponential_growth_lengthens_window() {
        let (gate, clock) = gate(Duration::from_secs(10), Duration::from_secs(600), 10);

        // First failure: 10s window.
        gate.record_failure(&"v1");
        clock.advance(Duration::from_secs(10));
        assert_eq!(gate.check(&"v1"), GateDecision::Proceed);

        // Second failure: 20s window — still suppressed after only 10s.
        gate.record_failure(&"v1");
        clock.advance(Duration::from_secs(10));
        assert_eq!(
            gate.check(&"v1"),
            GateDecision::InCooldown {
                consecutive_failures: 2
            }
        );
        clock.advance(Duration::from_secs(11));
        assert_eq!(gate.check(&"v1"), GateDecision::Proceed);
    }
}