// Common subagent modules
pub mod collection;
pub mod error;
pub mod logger;
pub mod opamp;
pub mod restart_policy;
pub mod values;

#[cfg(feature = "onhost")]
pub mod on_host;

#[cfg(feature = "k8s")]
pub mod k8s;

use std::thread::JoinHandle;

// CRATE TRAITS
use crate::config::super_agent_configs::AgentID;
use crate::config::super_agent_configs::SubAgentConfig;
use crate::event::event::Event;
use crate::event::EventPublisher;
use crate::opamp::callbacks::AgentCallbacks;

use self::logger::AgentLog;
use self::opamp::remote_config_publisher::SubAgentRemoteConfigPublisher;

pub(crate) type SubAgentCallbacks<P> = AgentCallbacks<SubAgentRemoteConfigPublisher<P>>;

/// The Runner trait defines the entry-point interface for a supervisor. Exposes a run method that will start the supervised processes' execution.
pub trait NotStartedSubAgent {
    type StartedSubAgent: StartedSubAgent;
    /// The run method will execute a supervisor (non-blocking). Returns a [`Stopper`] to manage the running process.
    fn run(self) -> Result<Self::StartedSubAgent, error::SubAgentError>;
}

// The Stopper trait defines the interface for a supervisor that is already running. Exposes a stop method that will stop the supervised processes' execution.
pub trait StartedSubAgent {
    /// Cancels the supervised process and returns its inner handle.
    fn stop(self) -> Result<Vec<JoinHandle<()>>, error::SubAgentError>;
}

pub trait SubAgentBuilder {
    type NotStartedSubAgent: NotStartedSubAgent;
    fn build(
        &self,
        agent_id: AgentID,
        sub_agent_config: &SubAgentConfig,
        tx: std::sync::mpsc::Sender<AgentLog>,
        ctx: impl EventPublisher<Event>,
    ) -> Result<Self::NotStartedSubAgent, error::SubAgentBuilderError>;
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::sub_agent::error::SubAgentBuilderError;
    use crate::sub_agent::error::SubAgentError::ErrorCreatingSubAgent;
    use mockall::{mock, predicate};

    mock! {
        pub StartedSubAgent {}

        impl StartedSubAgent for StartedSubAgent {
            fn stop(self) -> Result<Vec<JoinHandle<()>>, error::SubAgentError>;
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

            fn run(self) -> Result<<Self as NotStartedSubAgent>::StartedSubAgent, error::SubAgentError>;
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
                tx: std::sync::mpsc::Sender<AgentLog>,
                ctx: Context<Option<Event>>,
            ) -> Result<<Self as SubAgentBuilder>::NotStartedSubAgent, error::SubAgentBuilderError>;
        }
    }

    impl MockSubAgentBuilderMock {
        // should_build provides a helper method to create a subagent which runs and stops
        // successfully
        pub(crate) fn should_build(&mut self, times: usize) {
            self.expect_build().times(times).returning(|_, _, _, _| {
                let mut not_started_sub_agent = MockNotStartedSubAgent::new();
                not_started_sub_agent.expect_run().times(1).returning(|| {
                    let mut started_agent = MockStartedSubAgent::new();
                    started_agent
                        .expect_stop()
                        .times(1)
                        .returning(|| Ok(Vec::new()));
                    Ok(started_agent)
                });
                Ok(not_started_sub_agent)
            });
        }
        // should_build_running provides a helper method to create a Sub Agent which runs
        // successfully and does not stop
        pub(crate) fn should_build_running(
            &mut self,
            agent_id: &AgentID,
            sub_agent_config: SubAgentConfig,
        ) {
            self.expect_build()
                .once()
                .with(
                    predicate::eq(agent_id.clone()),
                    predicate::eq(sub_agent_config),
                    predicate::always(),
                    predicate::always(),
                )
                .returning(|_, _, _, _| {
                    let mut not_started_sub_agent = MockNotStartedSubAgent::new();
                    not_started_sub_agent
                        .expect_run()
                        .once()
                        .returning(|| Ok(MockStartedSubAgent::new()));
                    Ok(not_started_sub_agent)
                });
        }

        pub(crate) fn should_not_build(&mut self, times: usize) {
            self.expect_build().times(times).returning(|_, _, _, _| {
                Err(SubAgentBuilderError::SubAgent(ErrorCreatingSubAgent(
                    "error creating sub agent".to_string(),
                )))
            });
        }
    }
}
