use opamp_client::error::{ClientError, NotStartedClientError, StartedClientError};
use std::time::SystemTimeError;

use crate::config::error::SuperAgentConfigError;
use crate::config::remote_config_hash::HashRepositoryError;
use crate::opamp::client_builder::OpAMPClientBuilderError;
use crate::super_agent::effective_agents_assembler::EffectiveAgentsAssemblerError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SubAgentError {
    #[error("error creating Sub Agent: `{0}`")]
    ErrorCreatingSubAgent(String),
    #[error("Sub Agent `{0}` already exists")]
    AgentAlreadyExists(String),
    #[error("Sub Agent `{0}` not found")]
    AgentNotFound(String),
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

    #[cfg(feature = "onhost")]
    #[error("not started opamp client error: `{0}`")]
    SupervisorError(#[from] crate::sub_agent::on_host::supervisor::error::SupervisorError),

    #[error("remote config hash error: `{0}`")]
    RemoteConfigHashError(#[from] HashRepositoryError),
    #[error("super agent config error: `{0}`")]
    SuperAgentConfigError(#[from] SuperAgentConfigError),
    #[error("config assembler error: `{0}`")]
    ConfigAssemblerError(#[from] EffectiveAgentsAssemblerError),
}

#[derive(Error, Debug)]
pub enum SubAgentBuilderError {
    #[error("`{0}`")]
    SubAgent(#[from] SubAgentError),
    #[error("config assembler error: `{0}`")]
    ConfigAssemblerError(#[from] EffectiveAgentsAssemblerError),
    #[error("remote config hash error: `{0}`")]
    RemoteConfigHashError(#[from] HashRepositoryError),

    #[error("OpAMP client error error: `{0}`")]
    OpampClientBuilderError(#[from] OpAMPClientBuilderError),
    #[error("OpAMP client error error: `{0}`")]
    OpampClientError(#[from] ClientError),
}

#[derive(Error, Debug)]
pub enum SubAgentCollectionError {
    #[error("`{0}`")]
    SubAgent(#[from] SubAgentError),
    #[error("Sub Agent `{0}` not found in the collection")]
    SubAgentNotFound(String),
}
