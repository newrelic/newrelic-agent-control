use crate::event::channel::EventPublisher;
use crate::event::SubAgentInternalEvent;
use crate::sub_agent::error::SubAgentBuilderError;
use crate::sub_agent::health::health_checker::HealthCheckerError;
use crate::sub_agent::supervisor::stopper::SupervisorStopper;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SupervisorStarterError {
    #[error("the kube client returned an error: `{0}`")]
    Generic(#[from] crate::k8s::error::K8sError),

    #[error("building k8s resources: `{0}`")]
    ConfigError(String),

    #[error("building health checkers: `{0}`")]
    HealthError(#[from] HealthCheckerError),

    #[error("supervisor could not be built: `{0}`")]
    BuildError(#[from] SubAgentBuilderError),
}

pub trait SupervisorStarter {
    type SupervisorStopper: SupervisorStopper;

    fn start(
        self,
        sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>,
    ) -> Result<Self::SupervisorStopper, SupervisorStarterError>;
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::event::channel::EventPublisher;
    use crate::event::SubAgentInternalEvent;
    use crate::sub_agent::supervisor::starter::{SupervisorStarter, SupervisorStarterError};
    use crate::sub_agent::supervisor::stopper::tests::MockSupervisorStopper;
    use mockall::mock;
    use predicates::prelude::predicate;

    mock! {
        pub SupervisorStarter {}

         impl SupervisorStarter for SupervisorStarter{
             type SupervisorStopper = MockSupervisorStopper;
              fn start(self,sub_agent_internal_publisher: EventPublisher<SubAgentInternalEvent>) -> Result<MockSupervisorStopper, SupervisorStarterError>;
        }
    }

    impl MockSupervisorStarter {
        pub fn should_start(&mut self, stopper: MockSupervisorStopper) {
            self.expect_start()
                .with(predicate::always()) // we cannot do eq with a publisher
                .once()
                .return_once(|_| Ok(stopper));
        }
    }
}
