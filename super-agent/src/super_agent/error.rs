use opamp_client::error::{ClientError, NotStartedClientError, StartedClientError};
use resource_detection::DetectError;
use std::fmt::Debug;
use std::time::SystemTimeError;

use crate::config::agent_values::AgentValuesError;
use crate::config::persister::config_persister::PersistError;
use crate::file_reader::FileReaderError;
use crate::opamp::instance_id;
use crate::opamp::remote_config::RemoteConfigError;
use crate::opamp::remote_config_hash::HashRepositoryError;
use crate::sub_agent::error::{SubAgentBuilderError, SubAgentCollectionError, SubAgentError};
use crate::sub_agent::values::values_repository::ValuesRepositoryError;
use crate::super_agent::effective_agents_assembler::EffectiveAgentsAssemblerError;
use crate::super_agent::super_agent::EffectiveAgentsError;
use crate::{
    config::{
        agent_type::error::AgentTypeError, agent_type_registry::AgentRepositoryError,
        error::SuperAgentConfigError,
    },
    opamp::client_builder::OpAMPClientBuilderError,
};
use thiserror::Error;

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

    #[error("error getting agent ulid: `{0}`")]
    GetUlidError(#[from] instance_id::GetterError),

    #[error("`Effective agent error: {0}`")]
    EffectiveAgentsError(#[from] EffectiveAgentsError),

    #[error("`Sub Agent error: {0}`")]
    SubAgentError(#[from] SubAgentError),

    #[error("`{0}`")]
    SubAgentBuilder(#[from] SubAgentBuilderError),

    #[error("`{0}`")]
    SubAgentCollectionError(#[from] SubAgentCollectionError),

    #[error("system time error: `{0}`")]
    SystemTimeError(#[from] SystemTimeError),

    #[error("remote config hash error: `{0}`")]
    RemoteConfigHashError(#[from] HashRepositoryError),

    #[error("effective agents assembler error: `{0}`")]
    EffectiveAgentsAssemblerError(#[from] EffectiveAgentsAssemblerError),

    #[error("remote config error: `{0}`")]
    RemoteConfigError(#[from] RemoteConfigError),

    #[error("sub agent remote config error: `{0}`")]
    SubAgentRemoteConfigError(#[from] ValuesRepositoryError),

    #[error("sub agent values error: `{0}`")]
    SubAgentValuesError(#[from] AgentValuesError),

    #[error("External module error: `{0}`")]
    ExternalError(String),

    #[error("k8s config missing while running on k8s ")]
    K8sConfig(),

    #[error("resource detection error: `{0}`")]
    ResourceDetetctionError(#[from] DetectError),
}
