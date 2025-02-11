use std::sync::Arc;

use crate::event::channel::EventPublisher;
use crate::event::{SubAgentEvent, SubAgentInternalEvent};
use crate::sub_agent::error::SubAgentBuilderError;
use crate::sub_agent::health::health_checker::HealthCheckerError;
use crate::sub_agent::supervisor::stopper::SupervisorStopper;
use opamp_client::StartedClient;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SupervisorStarterError {
    #[cfg(feature = "k8s")]
    #[error("the kube client returned an error: `{0}`")]
    Generic(#[from] crate::k8s::error::K8sError),

    #[cfg(feature = "k8s")]
    #[error("building k8s resources: `{0}`")]
    ConfigError(String),

    #[error("building health checkers: `{0}`")]
    HealthError(#[from] HealthCheckerError),

    #[error("supervisor could not be built: `{0}`")]
    BuildError(#[from] SubAgentBuilderError),
}

pub trait SupervisorStarter<C>
where
    C: StartedClient + Send + Sync + 'static,
{
    type SupervisorStopper: SupervisorStopper;

    fn start(
        self,
        maybe_opamp_client: Arc<Option<C>>,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Result<Self::SupervisorStopper, SupervisorStarterError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use crate::event::channel::EventPublisher;
    use crate::event::{SubAgentEvent, SubAgentInternalEvent};
    use crate::sub_agent::supervisor::starter::{SupervisorStarter, SupervisorStarterError};
    use crate::sub_agent::supervisor::stopper::tests::MockSupervisorStopper;
    use mockall::mock;
    use opamp_client::StartedClient;

    mock! {
            pub SupervisorStarter<C> {}

             impl<C> SupervisorStarter<C>
             for SupervisorStarter<C>
    where
        C: StartedClient + Send + Sync + 'static,
             {
                 type SupervisorStopper = MockSupervisorStopper;
                  fn start<'a>(self,
            maybe_opamp_client: Arc<Option<C>>,
            sub_agent_publisher: EventPublisher<SubAgentEvent>,
                    sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>) -> Result<MockSupervisorStopper, SupervisorStarterError>;
            }
        }
}
