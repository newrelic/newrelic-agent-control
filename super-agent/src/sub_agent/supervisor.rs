use super::{
    effective_agents_assembler::{EffectiveAgent, EffectiveAgentsAssemblerError},
    error::SubAgentBuilderError,
};
use crate::event::channel::{EventPublisher, EventPublisherError};
use crate::event::SubAgentInternalEvent;
use crate::sub_agent::health::health_checker::HealthCheckerError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[cfg(feature = "k8s")]
    #[error("the kube client returned an error: `{0}`")]
    Generic(#[from] crate::k8s::error::K8sError),

    #[cfg(feature = "k8s")]
    #[error("building k8s resources: `{0}`")]
    ConfigError(String),

    #[error("building health checkers: `{0}`")]
    HealthError(#[from] HealthCheckerError),
}

pub trait SupervisorBuilder {
    type SupervisorStarter: SupervisorStarter;
    type OpAMPClient;

    fn build_supervisor(
        &self,
        effective_agent_result: Result<EffectiveAgent, EffectiveAgentsAssemblerError>,
        maybe_opamp_client: &Option<Self::OpAMPClient>,
    ) -> Result<Option<Self::SupervisorStarter>, SubAgentBuilderError>;
}

pub trait SupervisorStarter {
    type SupervisorStopper: SupervisorStopper;

    fn start(
        self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Result<Self::SupervisorStopper, SupervisorError>;
}

pub trait SupervisorStopper {
    fn stop(self) -> Result<(), EventPublisherError>;
}
