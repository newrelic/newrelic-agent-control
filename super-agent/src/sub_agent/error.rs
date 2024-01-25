use opamp_client::error::{ClientError, NotStartedClientError, StartedClientError};
use std::time::SystemTimeError;

use crate::agent_type::agent_values::AgentValuesError;
use crate::opamp::client_builder::OpAMPClientBuilderError;
use crate::opamp::remote_config_hash::HashRepositoryError;

use crate::event::channel::EventPublisherError;
use crate::opamp::remote_config::RemoteConfigError;
use crate::sub_agent::values::values_repository::ValuesRepositoryError;
use crate::super_agent::config::SuperAgentConfigError;
use thiserror::Error;

use super::effective_agents_assembler::EffectiveAgentsAssemblerError;

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

    #[cfg(all(not(feature = "onhost"), feature = "k8s"))]
    #[error("Supervisor run error: `{0}`")]
    SupervisorError(#[from] crate::sub_agent::k8s::SupervisorError),

    #[error("remote config hash error: `{0}`")]
    RemoteConfigHashError(#[from] HashRepositoryError),
    #[error("super agent config error: `{0}`")]
    SuperAgentConfigError(#[from] SuperAgentConfigError),
    #[error("config assembler error: `{0}`")]
    ConfigAssemblerError(#[from] EffectiveAgentsAssemblerError),

    #[error("sub agent values error: `{0}`")]
    ValuesError(#[from] ValuesRepositoryError),

    #[error("sub agent values error: `{0}`")]
    ValuesUnserializeError(#[from] AgentValuesError),

    #[error("remote config error: `{0}`")]
    RemoteConfigError(#[from] RemoteConfigError),

    #[error("Error publishing event: `{0}`")]
    EventPublisherError(#[from] EventPublisherError),

    #[error("Error handling thread: `{0}`")]
    PoisonError(String),
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

    #[error("unsupported K8s object: `{0}`")]
    UnsupportedK8sObject(String),

    #[error("Invalid configuration: `{0}`")]
    ConfigError(String),
}

#[derive(Error, Debug)]
pub enum SubAgentCollectionError {
    #[error("`{0}`")]
    SubAgent(#[from] SubAgentError),
    #[error("Sub Agent `{0}` not found in the collection")]
    SubAgentNotFound(String),
}
