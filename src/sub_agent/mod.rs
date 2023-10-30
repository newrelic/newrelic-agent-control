pub mod collection;
pub mod error;
pub mod k8s;
pub mod on_host;

use std::thread::JoinHandle;

// CRATE TRAITS
use mockall::automock;

use crate::{command::stream::Event, config::agent_type::agent_types::FinalAgent};

/// The Runner trait defines the entry-point interface for a supervisor. Exposes a run method that will start the supervised process' execution.
#[automock(type StartedSubAgent = MockStartedSubAgent;)]
pub trait NotStartedSubAgent {
    type StartedSubAgent: StartedSubAgent;

    /// The run method will execute a supervisor (non-blocking). Returns a [`StartedSubAgent`] to manage the running process.
    fn run(self) -> Result<Self::StartedSubAgent, error::SubAgentError>;
}

/// The Handle trait defines the interface for a supervised process' handle. Exposes a stop method that will cancel the supervised process' execution.
#[automock(type S =  ();)]
pub trait StartedSubAgent {
    /// Cancels the supervised process and returns its inner handle.
    fn stop(self) -> Result<Vec<JoinHandle<()>>, error::SubAgentError>;
}

pub trait SubAgentBuilder {
    type NotStartedSubAgent: NotStartedSubAgent;
    fn build(
        &self,
        agent: FinalAgent,
        tx: std::sync::mpsc::Sender<Event>,
    ) -> Result<Self::NotStartedSubAgent, error::SubAgentBuilderError>;
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use mockall::mock;

    mock! {
        pub SubAgentBuilderMock {}

        impl SubAgentBuilder for SubAgentBuilderMock {
            type NotStartedSubAgent = MockNotStartedSubAgent;

            fn build(
                &self,
                _agent: FinalAgent,
                _tx: std::sync::mpsc::Sender<Event>,
            ) -> Result<<Self as SubAgentBuilder>::NotStartedSubAgent, error::SubAgentBuilderError>;
        }
    }

    impl MockSubAgentBuilderMock {
        // should_build provides a helper method to create a subagent which runs and stops
        // successfully
        pub(crate) fn should_build(&mut self, times: usize) {
            self.expect_build().times(times).returning(|_, _| {
                let mut not_started_agent = MockNotStartedSubAgent::new();
                not_started_agent.expect_run().times(1).returning(|| {
                    let mut started_agent = MockStartedSubAgent::new();
                    started_agent
                        .expect_stop()
                        .times(1)
                        .returning(|| Ok(Vec::new()));
                    Ok(started_agent)
                });
                Ok(not_started_agent)
            });
        }

        // should_build provides a helper method to create a subagent which runs but doesn't stop
        pub(crate) fn should_build_and_run(&mut self, times: usize) {
            self.expect_build().times(times).returning(|_, _| {
                let mut not_started_agent = MockNotStartedSubAgent::new();
                not_started_agent
                    .expect_run()
                    .times(1)
                    .returning(|| Ok(MockStartedSubAgent::new()));
                Ok(not_started_agent)
            });
        }
    }
}
