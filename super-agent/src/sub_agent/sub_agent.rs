use crate::event::channel::EventPublisher;
use crate::event::SubAgentEvent;
use crate::opamp::callbacks::AgentCallbacks;
use crate::opamp::remote_config_publisher::OpAMPRemoteConfigPublisher;
use crate::sub_agent::error;
use crate::sub_agent::error::SubAgentError;
use crate::sub_agent::event_processor::SubAgentEventProcessor;
use crate::super_agent::config::{AgentID, AgentTypeFQN, SubAgentConfig};
use std::thread::JoinHandle;

pub(crate) type SubAgentCallbacks = AgentCallbacks<OpAMPRemoteConfigPublisher>;

/// The Runner trait defines the entry-point interface for a supervisor. Exposes a run method that will start the supervised processes' execution.
pub trait NotStartedSubAgent {
    type StartedSubAgent: StartedSubAgent;
    /// The run method will execute a supervisor (non-blocking). Returns a [`Stopper`] to manage the running process.
    fn run(self) -> Self::StartedSubAgent;
}

// The StartedSubAgent trait defines the interface for a supervisor that is already running.
// Exposes information about the Sub Agent and a stop method that will stop the
// supervised processes' execution.
pub trait StartedSubAgent {
    /// Returns the AgentID of the SubAgent
    fn agent_id(&self) -> AgentID;
    /// Returns the AgentType of the SubAgent
    fn agent_type(&self) -> AgentTypeFQN;
    /// Cancels the supervised process and returns its inner handle.
    fn stop(self) -> Result<Vec<JoinHandle<()>>, error::SubAgentError>;
}

pub trait SubAgentBuilder {
    type NotStartedSubAgent: NotStartedSubAgent;
    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        sub_agent_publisher: EventPublisher<SubAgentEvent>,
    ) -> Result<Self::NotStartedSubAgent, error::SubAgentBuilderError>;
}

////////////////////////////////////////////////////////////////////////////////////
// States for Started/Not Started Sub Agents
////////////////////////////////////////////////////////////////////////////////////
pub struct NotStarted<E>
where
    E: SubAgentEventProcessor,
{
    pub(crate) event_processor: E,
}

pub struct Started {
    pub(crate) event_loop_handle: JoinHandle<Result<(), SubAgentError>>,
}

#[cfg(test)]
pub mod test {
    use super::*;
    use mockall::{mock, predicate};

    mock! {
        pub StartedSubAgent {}

        impl StartedSubAgent for StartedSubAgent {
            fn stop(self) -> Result<Vec<JoinHandle<()>>, error::SubAgentError>;
            fn agent_id(&self) -> AgentID;
            fn agent_type(&self) -> AgentTypeFQN;
        }
    }

    impl MockStartedSubAgent {
        pub fn should_stop(&mut self) {
            self.expect_stop().once().returning(|| Ok(Vec::new()));
        }
    }

    mock! {
        pub NotStartedSubAgent {}

        impl NotStartedSubAgent for NotStartedSubAgent {
            type StartedSubAgent = MockStartedSubAgent;

            fn run(self) -> <Self as NotStartedSubAgent>::StartedSubAgent;
        }
    }

    impl MockNotStartedSubAgent {
        pub fn should_run(&mut self, started_sub_agent: MockStartedSubAgent) {
            self.expect_run()
                .once()
                .return_once(move || started_sub_agent);
        }
    }

    mock! {
        pub SubAgentBuilderMock {}

        impl SubAgentBuilder for SubAgentBuilderMock {
            type NotStartedSubAgent = MockNotStartedSubAgent;

            fn build(
                &self,
                agent_id: AgentID,
                sub_agent_config: &SubAgentConfig,
                sub_agent_publisher: EventPublisher<SubAgentEvent>,
            ) -> Result<<Self as SubAgentBuilder>::NotStartedSubAgent, error::SubAgentBuilderError>;
        }
    }

    impl MockSubAgentBuilderMock {
        // should_build provides a helper method to create a subagent which runs and stops
        // successfully
        pub(crate) fn should_build(&mut self, times: usize) {
            self.expect_build().times(times).returning(|_, _, _| {
                let mut not_started_sub_agent = MockNotStartedSubAgent::new();
                not_started_sub_agent.expect_run().times(1).returning(|| {
                    let mut started_agent = MockStartedSubAgent::new();
                    started_agent
                        .expect_stop()
                        .times(1)
                        .returning(|| Ok(Vec::new()));
                    started_agent
                });
                Ok(not_started_sub_agent)
            });
        }

        pub(crate) fn should_build_not_started(
            &mut self,
            agent_id: &AgentID,
            sub_agent_config: SubAgentConfig,
            sub_agent: MockNotStartedSubAgent,
        ) {
            self.expect_build()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(sub_agent_config),
                    predicate::always(),
                )
                .return_once(move |_, _, _| Ok(sub_agent));
        }
    }
}
