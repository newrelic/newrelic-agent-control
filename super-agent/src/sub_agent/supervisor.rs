use super::{effective_agents_assembler::EffectiveAgent, error::SubAgentBuilderError};
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

    #[error("supervisor builder error: `{0}`")]
    StartError(#[from] SubAgentBuilderError),
}

pub trait SupervisorBuilder {
    type SupervisorStarter: SupervisorStarter;
    type OpAMPClient;

    fn build_supervisor(
        &self,
        effective_agent: EffectiveAgent,
    ) -> Result<Self::SupervisorStarter, SubAgentBuilderError>;
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

#[cfg(test)]
pub(crate) mod test {
    use crate::event::channel::{EventPublisher, EventPublisherError};
    use crate::event::SubAgentInternalEvent;
    use crate::opamp::callbacks::AgentCallbacks;
    use crate::opamp::client_builder::test::MockStartedOpAMPClientMock;
    use crate::opamp::effective_config::loader::tests::MockEffectiveConfigLoaderMock;
    use crate::sub_agent::effective_agents_assembler::EffectiveAgent;
    use crate::sub_agent::error::SubAgentBuilderError;
    use crate::sub_agent::supervisor::{SupervisorBuilder, SupervisorStopper};
    use crate::sub_agent::supervisor::{SupervisorError, SupervisorStarter};
    use mockall::mock;

    mock! {
        pub SupervisorStopper {}

        impl SupervisorStopper for SupervisorStopper{
        fn stop(self) -> Result<(), EventPublisherError>;
        }
    }

    mock! {
        pub SupervisorStarter {}

         impl SupervisorStarter for SupervisorStarter{
             type SupervisorStopper= MockSupervisorStopper;
              fn start(self,sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>) -> Result<MockSupervisorStopper, SupervisorError>;
        }
    }

    mock! {
        pub SupervisorBuilder<A> where A: SupervisorStarter {}

        impl<A> SupervisorBuilder for SupervisorBuilder<A> where A: SupervisorStarter {
            type SupervisorStarter = A;
            type OpAMPClient = MockStartedOpAMPClientMock<AgentCallbacks<MockEffectiveConfigLoaderMock>>;

            fn build_supervisor(
                &self,
                effective_agent: EffectiveAgent,
            ) -> Result<A, SubAgentBuilderError>;
        }
    }
}
