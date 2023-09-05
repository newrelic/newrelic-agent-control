use std::fmt::Debug;

use thiserror::Error;

use crate::config::{
    agent_type::AgentTypeError, agent_type_registry::AgentRepositoryError,
    error::SuperAgentConfigError,
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

    #[error("agent runner config error")]
    SupervisorGroupError,
}
