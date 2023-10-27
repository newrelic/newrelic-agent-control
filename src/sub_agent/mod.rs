pub mod collection;
pub mod error;
pub mod on_host;

use std::thread::JoinHandle;

// CRATE TRAITS
use mockall::automock;

use crate::{
    command::stream::Event, config::agent_type::agent_types::FinalAgent,
    opamp::client_builder::OpAMPClientBuilder, super_agent::instance_id::InstanceIDGetter,
};

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
    fn build<OpAMPBuilder, ID>(
        &self,
        agent: FinalAgent,
        tx: std::sync::mpsc::Sender<Event>,
        opamp_builder: Option<&OpAMPBuilder>,
        instance_id_getter: &ID,
    ) -> Result<Self::NotStartedSubAgent, error::SubAgentBuilderError>
    where
        OpAMPBuilder: OpAMPClientBuilder,
        ID: InstanceIDGetter;
}

pub struct MockSubAgentBuilder;

impl MockSubAgentBuilder {
    pub fn new() -> Self {
        MockSubAgentBuilder
    }
}

impl SubAgentBuilder for MockSubAgentBuilder {
    type NotStartedSubAgent = MockNotStartedSubAgent;
    fn build<OpAMPBuilder, ID>(
        &self,
        _agent: FinalAgent,
        _tx: std::sync::mpsc::Sender<Event>,
        _opamp_builder: Option<&OpAMPBuilder>,
        _instance_id_getter: &ID,
    ) -> Result<Self::NotStartedSubAgent, error::SubAgentBuilderError>
    where
        OpAMPBuilder: OpAMPClientBuilder,
        ID: InstanceIDGetter,
    {
        Ok(MockNotStartedSubAgent::new())
    }
}
