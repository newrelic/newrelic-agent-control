use crate::utils::retry::BackoffPolicy;
use crate::utils::time::{Clock, SystemClock};
use std::sync::Mutex;
use std::time::Instant;

/// Why the gate withheld an attempt. This is the suppressed outcome of [`BackoffGate::check`]
/// (which returns `None` to mean "proceed") and the error type of [`BackoffGate::guarded`], so a
/// "proceed" case can never appear on the "ran the operation" path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressionReason {
    /// The backoff cooldown window after the last failure has not elapsed yet.
    InCooldown { consecutive_failures: u32 },
    /// Suppressed within the current backoff window, with the `max_attempts` consecutive-failure
    /// threshold crossed. The gate keeps probing once the window elapses; this variant only
    /// escalates how the suppression is reported.
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

/// A thread-safe gate that throttles repeated attempts at a fallible operation
/// keyed by `K` (e.g. a target version
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

    /// Inspects the gate for `key`: returns `None` when the caller should proceed, or
    /// `Some(reason)` when the attempt should be suppressed.
    ///
    /// If `key` differs from the last-seen key the gate resets first, so switching targets
    /// always permits an immediate attempt.
    pub fn check(&self, key: &K) -> Option<SuppressionReason> {
        let mut state = self.state.lock().expect("backoff-gate lock poisoned");
        Self::reset_if_key_changed(&mut state, key);

        // Within the current backoff window → suppress, escalating the reported reason once the
        // consecutive-failure threshold has been crossed.
        match state.next_attempt_at {
            Some(t)
                if self.clock.now() < t
                    && (state.consecutive_failures as usize) >= self.policy.max_attempts.get() =>
            {
                Some(SuppressionReason::CapReached {
                    consecutive_failures: state.consecutive_failures,
                })
            }
            Some(t) if self.clock.now() < t => Some(SuppressionReason::InCooldown {
                consecutive_failures: state.consecutive_failures,
            }),
            // Outside the backoff window (or no attempt scheduled): proceed with the operation.
            _ => None,
        }
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

    /// The key the gate is currently tracking (the last key seen via [`check`](Self::check) or
    /// [`record_failure`](Self::record_failure)), or `None` if the gate is unused or was reset.
    pub fn current_key(&self) -> Option<K> {
        self.state
            .lock()
            .expect("backoff-gate lock poisoned")
            .key
            .clone()
    }

    /// Runs `op` for `key` through the gate.
    ///
    /// When the gate permits an attempt the operation runs: the gate is [reset](Self::reset) on
    /// `Ok` and a failure is [recorded](Self::record_failure) on `Err`, and the operation's own
    /// result is returned wrapped in `Ok`. When the gate is in cooldown or has reached its cap,
    /// `op` is not run and the [`SuppressionReason`] verdict is returned as `Err`.
    pub fn guarded<T, E, F>(&self, key: &K, op: F) -> Result<Result<T, E>, SuppressionReason>
    where
        F: FnOnce() -> Result<T, E>,
    {
        match self.check(key) {
            None => {
                let result = op();
                if result.is_ok() {
                    self.reset();
                } else {
                    self.record_failure(key);
                }
                Ok(result)
            }
            Some(suppression) => Err(suppression),
        }
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
    use std::num::NonZeroUsize;
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
            max_attempts: NonZeroUsize::new(max_attempts)
                .expect("test policy max_attempts non-zero"),
            base_delay: base,
            max_delay: max,
            jitter: false,
        }
    }

    fn gate(
        base: Duration,
        max: Duration,
        max_attempts: usize,
    ) -> (BackoffGate<&'static str, FakeClock>, FakeClock) {
        let clock = FakeClock::new(Instant::now());
        let gate = BackoffGate::new(policy(base, max, max_attempts), clock.clone());
        (gate, clock)
    }

    #[test]
    fn proceeds_when_no_failures() {
        let (gate, _clock) = gate(Duration::from_secs(1), Duration::from_secs(10), 5);
        assert_eq!(gate.check(&"v1"), None);
    }

    #[test]
    fn suppresses_within_cooldown_window_then_re_attempts() {
        let (gate, clock) = gate(Duration::from_secs(30), Duration::from_secs(600), 5);

        assert_eq!(gate.check(&"v1"), None);
        gate.record_failure(&"v1");

        assert_eq!(
            gate.check(&"v1"),
            Some(SuppressionReason::InCooldown {
                consecutive_failures: 1
            })
        );

        // base_delay is 30s with no jitter; advancing past it re-opens the gate.
        clock.advance(Duration::from_secs(31));
        assert_eq!(gate.check(&"v1"), None);
    }

    #[test]
    fn cap_is_reported_but_not_terminal() {
        let (gate, clock) = gate(Duration::from_secs(30), Duration::from_secs(30), 2);

        // One failure: below the cap, suppressed only within the window.
        gate.record_failure(&"v1");
        assert_eq!(
            gate.check(&"v1"),
            Some(SuppressionReason::InCooldown {
                consecutive_failures: 1
            })
        );
        clock.advance(Duration::from_secs(31));
        assert_eq!(gate.check(&"v1"), None);

        // Second failure: cap reached — reported as CapReached *within* the window...
        gate.record_failure(&"v1");
        assert_eq!(
            gate.check(&"v1"),
            Some(SuppressionReason::CapReached {
                consecutive_failures: 2
            })
        );

        // ...but the cap is not terminal: once the window elapses the gate probes again.
        clock.advance(Duration::from_secs(31));
        assert_eq!(gate.check(&"v1"), None);
    }

    #[test]
    fn current_key_tracks_last_key_and_clears_on_reset() {
        let (gate, _clock) = gate(Duration::from_secs(60), Duration::from_secs(600), 5);

        assert_eq!(gate.current_key(), None);
        gate.record_failure(&"v1");
        assert_eq!(gate.current_key(), Some("v1"));
        gate.reset();
        assert_eq!(gate.current_key(), None);
    }

    #[test]
    fn key_change_resets_and_allows_immediate_attempt() {
        let (gate, _clock) = gate(Duration::from_secs(60), Duration::from_secs(600), 5);

        gate.record_failure(&"v1");
        assert_eq!(
            gate.check(&"v1"),
            Some(SuppressionReason::InCooldown {
                consecutive_failures: 1
            })
        );
        // A different key clears the cooldown without advancing the clock.
        assert_eq!(gate.check(&"v2"), None);
    }

    #[test]
    fn reset_clears_failure_state() {
        let (gate, _clock) = gate(Duration::from_secs(60), Duration::from_secs(600), 5);

        gate.record_failure(&"v1");
        gate.reset();
        assert_eq!(gate.check(&"v1"), None);
    }

    #[test]
    fn exponential_growth_lengthens_window() {
        let (gate, clock) = gate(Duration::from_secs(10), Duration::from_secs(600), 10);

        // First failure: 10s window.
        gate.record_failure(&"v1");
        clock.advance(Duration::from_secs(10));
        assert_eq!(gate.check(&"v1"), None);

        // Second failure: 20s window — still suppressed after only 10s.
        gate.record_failure(&"v1");
        clock.advance(Duration::from_secs(10));
        assert_eq!(
            gate.check(&"v1"),
            Some(SuppressionReason::InCooldown {
                consecutive_failures: 2
            })
        );
        clock.advance(Duration::from_secs(11));
        assert_eq!(gate.check(&"v1"), None);
    }
}
