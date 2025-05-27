use super::effective_agents_assembler::EffectiveAgentsAssemblerError;
use super::remote_config_parser::RemoteConfigParserError;
use super::supervisor::starter::SupervisorStarterError;
use crate::event::channel::EventPublisherError;
use crate::opamp::client_builder::OpAMPClientBuilderError;
use crate::values::config_repository::ConfigRepositoryError;
use opamp_client::StartedClientError;
use opamp_client::{ClientError, NotStartedClientError};
use std::time::SystemTimeError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SubAgentError {
    #[error("system time error: `{0}`")]
    SystemTimeError(#[from] SystemTimeError),
    #[error("OpAMP client error: `{0}`")]
    OpampClientError(#[from] ClientError),
    #[error("OpAMP client error: `{0}`")]
    OpampClientBuilderError(#[from] OpAMPClientBuilderError),
    #[error("started opamp client error: `{0}`")]
    StartedOpampClientError(#[from] StartedClientError),
    #[error("not started opamp client error: `{0}`")]
    NotStartedOpampClientError(#[from] NotStartedClientError),
    #[error("config assembler error: `{0}`")]
    ConfigAssemblerError(#[from] EffectiveAgentsAssemblerError),
    #[error("sub agent yaml config repository error: `{0}`")]
    ConfigRepositoryError(#[from] ConfigRepositoryError),
    #[error("Error publishing event: `{0}`")]
    EventPublisherError(#[from] EventPublisherError),
    #[error("no configuration found")]
    NoConfiguration,
}

/// Errors that can occur when creating a supervisor, including when receiving a remote config.
#[derive(Error, Debug)]
pub enum SupervisorCreationError {
    #[error("failed remote config hash for remote config: `{0}`")]
    RemoteConfigHash(String),
    #[error("could not parse the remote config: `{0}`")]
    RemoteConfigParse(#[from] RemoteConfigParserError),
    #[error("no configuration found")]
    NoConfiguration,
    #[error("could not assemble the effective agent from YAML config: `{0}`")]
    EffectiveAgentAssemble(#[from] EffectiveAgentsAssemblerError),
    #[error("could not build the supervisor from an effective agent: `{0}`")]
    SupervisorAssemble(#[from] SubAgentBuilderError),
    #[error("could not start the supervisor: `{0}`")]
    SupervisorStart(#[from] SupervisorStarterError),
}

#[derive(Error, Debug)]
pub enum SubAgentBuilderError {
    #[error("`{0}`")]
    SubAgent(#[from] SubAgentError),
    #[error("config assembler error: `{0}`")]
    ConfigAssemblerError(#[from] EffectiveAgentsAssemblerError),
    #[error("OpAMP client error: `{0}`")]
    OpampClientBuilderError(#[from] OpAMPClientBuilderError),
    #[error("unsupported K8s object: `{0}`")]
    UnsupportedK8sObject(String),
}

#[derive(Error, Debug)]
pub enum SubAgentCollectionError {
    #[error("`{0}`")]
    SubAgent(#[from] SubAgentError),
    #[error("Sub Agent `{0}` not found in the collection")]
    SubAgentNotFound(String),
}

#[derive(Error, Debug)]
pub enum SubAgentStopError {
    #[error("could not stop the sub agent event loop: `{0}`")]
    SubAgentEventLoop(#[from] EventPublisherError),
    #[error("failed to join the sub agent thread: `{0}")]
    SubAgentJoinHandle(String),
    #[error("failed to stop the sub agent runtime: `{0}")]
    SubAgentRuntimeStop(#[from] SubAgentError),
}
