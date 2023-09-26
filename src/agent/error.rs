use std::fmt::Debug;

use thiserror::Error;

use crate::file_reader::FileReaderError;
use crate::{
    config::{
        agent_type::AgentTypeError, agent_type_registry::AgentRepositoryError,
        error::SuperAgentConfigError,
    },
    opamp::client_builder::OpAMPClientBuilderError,
};

use super::supervisor_group::SupervisorGroupError;

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
}
