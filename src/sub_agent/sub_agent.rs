use opamp_client::error::{ClientError, NotStartedClientError, StartedClientError};
use std::fmt::Debug;
use std::thread::JoinHandle;
use std::time::SystemTimeError;

use crate::opamp::client_builder::OpAMPClientBuilderError;
use crate::supervisor::error::SupervisorError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SubAgentError {
    #[error("error creating Sub Agent: `{0}`")]
    ErrorCreatingSubAgent(String),
    #[error("Sub Agent `{0}` already exists")]
    AgentAlreadyExists(String),
    #[error("system time error: `{0}`")]
    SystemTimeError(#[from] SystemTimeError),
    #[error("OpAMP client error error: `{0}`")]
    OpampClientError(#[from] ClientError),
    #[error("OpAMP client error error: `{0}`")]
    OpampClientBuilderError(#[from] OpAMPClientBuilderError),
    #[error("started opamp client error: `{0}`")]
    StartedOpampClientError(#[from] StartedClientError),
    #[error("not started opamp client error: `{0}`")]
    NotStartedOpampClientError(#[from] NotStartedClientError),
    #[error("not started opamp client error: `{0}`")]
    SupervisorError(#[from] SupervisorError),
}

/// The Runner trait defines the entry-point interface for a supervisor. Exposes a run method that will start the supervised process' execution.
pub trait NotStartedSubAgent {
    type StartedSubAgent: StartedSubAgent;

    /// The run method will execute a supervisor (non-blocking). Returns a [`StartedSubAgent`] to manage the running process.
    fn run(self) -> Result<Self::StartedSubAgent, SubAgentError>;
}

/// The Handle trait defines the interface for a supervised process' handle. Exposes a stop method that will cancel the supervised process' execution.
pub trait StartedSubAgent {
    /// Cancels the supervised process and returns its inner handle.
    fn stop(self) -> Result<Vec<JoinHandle<()>>, SubAgentError>;
}
