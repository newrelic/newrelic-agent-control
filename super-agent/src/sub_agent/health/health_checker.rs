use std::time::Duration;

pub(super) type HealthCheckError = String;

/// A type that implements a health checking mechanism.
pub trait HealthChecker: Send {
    /// Check the health of the agent. `Ok(())` means the agent is healthy. Otherwise,
    /// we will have an `Err(e)` where `e` is the error with agent-specific semantics
    /// with which we will build the OpAMP's `ComponentHealth.status` contents.
    /// See OpAMP's [spec](https://github.com/open-telemetry/opamp-spec/blob/main/specification.md#componenthealthstatus)
    /// for more details.
    fn check_health(&self) -> Result<(), HealthCheckError>;

    fn interval(&self) -> Duration;
}
