use opamp_client::error::{ClientError, NotStartedClientError, StartedClientError};
use std::fmt::Debug;

use thiserror::Error;

use super::supervisor_group::SupervisorGroupError;
use crate::agent::EffectiveAgentsError;
use crate::config::persister::config_persister::PersistError;
use crate::file_reader::FileReaderError;
use crate::{
    config::{
        agent_type::error::AgentTypeError, agent_type_registry::AgentRepositoryError,
        error::SuperAgentConfigError,
    },
    opamp::client_builder::OpAMPClientBuilderError,
};

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("channel is not present in the agent initializer")]
    ChannelExtractError,

    #[error("could not resolve config: `{0}`")]
    ConfigResolveError(#[from] SuperAgentConfigError),

    #[error("agent repository error: `{0}`")]
    AgentRepositoryError(#[from] AgentRepositoryError),

    #[error("filesystem error: `{0}`")]
    FileSystemError(#[from] std::io::Error),

    #[error("error deserializing YAML: `{0}`")]
    SerdeYaml(#[from] serde_yaml::Error),

    #[error("agent type error `{0}`")]
    AgentTypeError(#[from] AgentTypeError),

    #[error("`{0}`")]
    OpAMPBuilderError(#[from] OpAMPClientBuilderError),

    #[error("`{0}`")]
    SupervisorGroupError(#[from] SupervisorGroupError),

    #[error("file reader error: `{0}`")]
    FileReaderError(#[from] FileReaderError),

    #[error("`{0}`")]
    OpAMPClientError(#[from] ClientError),

    #[error("`{0}`")]
    OpAMPNotStartedClientError(#[from] NotStartedClientError),

    #[error("`{0}`")]
    OpAMPStartedClientError(#[from] StartedClientError),

    #[error("error persisting agent config: `{0}`")]
    PersistError(#[from] PersistError),

    #[error("`Effective agent error{0}`")]
    EffectiveAgentsError(#[from] EffectiveAgentsError),
}
