//! Supervisor traits for managing sub-agent lifecycle.
//!
//! This module defines a three-phase pattern for supervisor implementations that manage
//! sub-agents. Each phase is represented by a trait with clear ownership semantics:
//!
//! 1. [SupervisorBuilder] - Creates supervisors from configuration
//! 2. [SupervisorStarter] - Initializes resources and launches the supervisor
//! 3. [Supervisor] - Manages the running agent and handles runtime updates
//!
//! # Usage
//!
//! ```rust,ignore
//! // Build
//! let supervisor_starter = builder.build_supervisor(effective_agent)?;
//!
//! // Start
//! let supervisor = supervisor_starter.start(event_publisher)?;
//!
//! // Manage
//! let supervisor = supervisor.apply(new_config)?;  // Update configuration
//! // ...
//! supervisor.stop()?; // Cleanup
//! ```

use thiserror::Error;

use crate::{
    event::{SubAgentInternalEvent, channel::EventPublisher},
    sub_agent::effective_agents_assembler::EffectiveAgent,
};

use std::{error::Error, marker::Sized};

// TODO: the traits in these modules will be replaced by the ones defined there.
pub mod builder;
pub mod starter;
pub mod stopper;

/// Constructs a supervisor for managing sub-agent lifecycle.
///
/// This trait is responsible for building a supervisor starter based on an effective agent
/// configuration.
///
/// # Type Parameters
///
/// * `Starter` - The type representing a not-started supervisor ready to be started.
/// * `Error` - The error type returned when building the starter fails
pub trait SupervisorBuilder {
    type Starter: SupervisorStarter;
    type Error: Error;

    /// Builds a supervisor starter from the given effective agent configuration.
    ///
    /// # Arguments
    ///
    /// * `effective_agent` - The desired agent configuration to supervise
    ///
    /// # Returns
    ///
    /// * `Ok(Self::Starter)` - A starter ready to launch the supervisor
    /// * `Err(Self::Error)` - If the configuration is invalid or resources cannot be prepared
    fn build_supervisor(
        &self,
        effective_agent: EffectiveAgent,
    ) -> Result<Self::Starter, Self::Error>;
}

/// Launches a supervisor and returns a handle for managing it.
///
/// It takes ownership of the starter, performs the necessary initialization steps (such as
/// creating filesystem entries, setting up health checkers, or deploying Kubernetes resources),
/// and returns a running supervisor.
///
/// # Type Parameters
///
/// * `Supervisor` - The type representing the running supervisor
/// * `Error` - The error type returned when starting the supervisor fails
pub trait SupervisorStarter {
    type Supervisor: Supervisor;
    type Error: Error;

    /// Starts the supervisor, consuming this starter.
    ///
    /// This method performs all necessary initialization steps to get the supervisor running,
    /// such as creating filesystem entries, launching background threads, deploying Kubernetes
    /// resources, or setting up health checks.
    ///
    /// # Arguments
    ///
    /// * `sub_agent_internal_publisher` - Event publisher for internal sub-agent events
    ///
    /// # Returns
    ///
    /// * `Ok(Self::Supervisor)` - A running supervisor that can be managed and stopped
    /// * `Err(Self::Error)` - If startup fails (e.g., resource creation fails, deployment fails)
    fn start(
        self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Result<Self::Supervisor, Self::Error>;
}

/// Manages a running sub-agent supervisor, handling configuration updates and shutdown.
///
/// A supervisor is responsible for ensuring the sub-agent matches the desired configuration and
/// for gracefully shutting down when requested.
///
/// The `apply` method takes ownership of `self` and returns a new instance. This design
/// ensures that configuration changes are atomic from the caller's perspective and allows
/// implementations to potentially replace internal state or recreate resources as needed.
///
/// # Type Parameters
///
/// * `ApplyError` - The error type returned when applying configuration changes fails
/// * `StopError` - The error type returned when stopping fails
pub trait Supervisor: Sized {
    type ApplyError: Error;
    type StopError: Error;

    /// Applies a new effective agent configuration to the running supervisor.
    ///
    /// This method updates the supervisor to match the provided configuration. Depending on
    /// the implementation, this may involve updating Kubernetes resources, restarting processes,
    /// or reconfiguring health checks.
    ///
    /// The method consumes `self`, allowing implementations to handle configuration changes atomically and either
    /// return the same instance of drop the consumed [Supervisor] and return a new one.
    ///
    /// # Arguments
    ///
    /// * `effective_agent` - The new desired agent configuration to apply
    ///
    /// # Returns
    ///
    /// * `Ok(Self)` - A supervisor instance with the new configuration applied
    /// * `Err(Self::ApplyError)` - If there is an error applying the configuration.
    ///
    fn apply(self, effective_agent: EffectiveAgent) -> Result<Self, Self::ApplyError>;

    /// Stops the supervisor and cleans up all associated resources.
    ///
    /// This method performs a graceful shutdown, stopping managed processes, cleaning up
    /// filesystem entries, deleting Kubernetes resources, or joining background threads as
    /// appropriate for the implementation.
    ///
    /// # Returns
    ///
    /// * `Ok(())` - The supervisor was successfully stopped
    /// * `Err(Self::StopError)` - If shutdown encountered errors (resources may be partially cleaned)
    fn stop(self) -> Result<(), Self::StopError>;
}
#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::event::{SubAgentInternalEvent, channel::EventPublisher};
    use mockall::mock;
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[error("mock error: {0}")]
    pub struct MockError(String);

    impl From<String> for MockError {
        fn from(value: String) -> Self {
            Self(value)
        }
    }

    mock! {
        pub Supervisor {}
        impl Supervisor for Supervisor {
            type StopError = MockError;

            fn apply(self, effective_agent: EffectiveAgent) -> Result<Self, ApplyError<Self>>;
            fn stop(self) -> Result<(), <Self as Supervisor>::StopError>;
        }
    }

    mock! {
        pub SupervisorStarter<S> where S: Supervisor + 'static {}
        impl<S> SupervisorStarter for SupervisorStarter<S> where S: Supervisor + 'static {
            type Supervisor = S;
            type Error = MockError;

            fn start(
                self,
                sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
            ) -> Result<S, <Self as SupervisorStarter>::Error>;
        }
    }

    mock! {
        pub SupervisorBuilder<S> where S: SupervisorStarter + 'static {}
        impl<S> SupervisorBuilder for SupervisorBuilder<S> where S: SupervisorStarter + 'static {
            type Starter = S;
            type Error = MockError;

            fn build_supervisor(
                &self,
                effective_agent: EffectiveAgent,
            ) -> Result<S, <Self as SupervisorBuilder>::Error>;
        }
    }
}
