// Common subagent modules
pub mod collection;
pub mod error;
pub mod logger;
pub mod restart_policy;

#[cfg(feature = "onhost")]
pub mod on_host;

#[cfg(feature = "k8s")]
pub mod k8s;

use std::thread::JoinHandle;

// CRATE TRAITS
use crate::config::{agent_type::agent_types::FinalAgent, super_agent_configs::AgentID};

use self::logger::Event;

/// The Runner trait defines the entry-point interface for a supervisor. Exposes a run method that will start the supervised process' execution.
pub trait SubAgent {
    /// The run method will execute a supervisor (non-blocking). Returns a [`SubAgent`] to manage the running process.
    //TODO : DO WITH A GENERIC TYPE AND NOT CONSUME HIMSELF
    fn run(&mut self) -> Result<(), error::SubAgentError>;

    /// Cancels the supervised process and returns its inner handle.
    fn stop(self) -> Result<Vec<JoinHandle<()>>, error::SubAgentError>;
}

pub trait SubAgentBuilder {
    type SubAgent: SubAgent;
    fn build(
        &self,
        agent: FinalAgent,
        agent_id: AgentID,
        tx: std::sync::mpsc::Sender<Event>,
    ) -> Result<Self::SubAgent, error::SubAgentBuilderError>;
}

#[cfg(test)]
pub mod test {
    use super::*;
    use mockall::mock;

    mock! {
        pub SubAgent {}

        impl SubAgent for SubAgent {

            fn stop(self) -> Result<Vec<JoinHandle<()>>, error::SubAgentError>;

            fn run(&mut self) -> Result<(), error::SubAgentError>;
        }
    }

    mock! {
        pub SubAgentBuilderMock {}

        impl SubAgentBuilder for SubAgentBuilderMock {
            type SubAgent = MockSubAgent;

            fn build(
                &self,
                _agent: FinalAgent,
                _agent_id: AgentID,
                _tx: std::sync::mpsc::Sender<Event>,
            ) -> Result<<Self as SubAgentBuilder>::SubAgent, error::SubAgentBuilderError>;
        }
    }

    impl MockSubAgentBuilderMock {
        // should_build provides a helper method to create a subagent which runs and stops
        // successfully
        pub(crate) fn should_build(&mut self, times: usize) {
            self.expect_build().times(times).returning(|_, _, _| {
                let mut sub_agent = MockSubAgent::new();
                sub_agent.expect_run().times(1).returning(|| Ok(()));
                sub_agent.expect_stop().times(1).returning(|| Ok(Vec::new()));
                Ok(sub_agent)
            });
        }
    }
}
