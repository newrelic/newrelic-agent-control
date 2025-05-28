use super::config::AgentControlConfigError;
use super::resource_cleaner::ResourceCleanerError;
use super::updater::UpdaterError;
use crate::agent_control::config_validator::DynamicConfigValidatorError;
use crate::agent_type::agent_type_registry::AgentRepositoryError;
use crate::agent_type::error::AgentTypeError;
use crate::agent_type::render::persister::config_persister::PersistError;
use crate::event::channel::EventPublisherError;
use crate::opamp::client_builder::OpAMPClientBuilderError;
use crate::opamp::hash_repository::repository::HashRepositoryError;
use crate::opamp::instance_id;

use crate::opamp::instance_id::on_host::getter::IdentifiersProviderError;
use crate::opamp::remote_config::RemoteConfigError;
use crate::sub_agent::effective_agents_assembler::EffectiveAgentsAssemblerError;
use crate::sub_agent::error::{SubAgentBuilderError, SubAgentCollectionError, SubAgentError};
use crate::values::yaml_config::YAMLConfigError;
use crate::values::yaml_config_repository::YAMLConfigRepositoryError;
use fs::file_reader::FileReaderError;
use opamp_client::{ClientError, NotStartedClientError, StartedClientError};
use std::fmt::Debug;
use std::time::SystemTimeError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("could not resolve config: `{0}`")]
    ConfigResolve(#[from] AgentControlConfigError),

    #[error("agent repository error: `{0}`")]
    AgentRepository(#[from] AgentRepositoryError),

    #[error("filesystem error: `{0}`")]
    FileSystem(#[from] std::io::Error),

    #[error("error deserializing YAML: `{0}`")]
    SerdeYaml(#[from] serde_yaml::Error),

    #[error("agent type error `{0}`")]
    AgentType(#[from] AgentTypeError),

    #[error("`{0}`")]
    OpAMPBuilder(#[from] OpAMPClientBuilderError),

    #[error("file reader error: `{0}`")]
    FileReader(#[from] FileReaderError),

    #[error("`{0}`")]
    OpAMPClient(#[from] ClientError),

    #[error("`{0}`")]
    OpAMPNotStartedClient(#[from] NotStartedClientError),

    #[error("`{0}`")]
    OpAMPStartedClient(#[from] StartedClientError),

    #[error("error persisting agent config: `{0}`")]
    Persistence(#[from] PersistError),

    #[error("error getting agent instance id: `{0}`")]
    GetInstanceID(#[from] instance_id::getter::GetterError),

    #[error("`Sub Agent error: {0}`")]
    SubAgent(#[from] SubAgentError),

    #[error("`{0}`")]
    SubAgentBuilder(#[from] SubAgentBuilderError),

    #[error("`{0}`")]
    SubAgentCollection(#[from] SubAgentCollectionError),

    #[error("system time error: `{0}`")]
    SystemTime(#[from] SystemTimeError),

    #[error("remote config hash error: `{0}`")]
    RemoteConfigHash(#[from] HashRepositoryError),

    #[error("effective agents assembler error: `{0}`")]
    EffectiveAgentsAssembler(#[from] EffectiveAgentsAssemblerError),

    #[error("remote config error: `{0}`")]
    RemoteConfig(#[from] RemoteConfigError),

    #[error("sub agent remote config error: `{0}`")]
    SubAgentRemoteConfig(#[from] YAMLConfigRepositoryError),

    #[error("External module error: `{0}`")]
    ExternalError(String),

    #[error("error from http client: `{0}`")]
    Http(String),

    #[error("required identifiers error: `{0}`")]
    Identifiers(String),

    #[error("error publishing event: `{0}`")]
    EventPublisher(#[from] EventPublisherError),

    #[error("parsing remote config into YAMLConfig: `{0}`")]
    YAMLConfig(#[from] YAMLConfigError),

    #[error("failed to initialize the identifiers provider: `{0}`")]
    InitializeIdentifiersProvider(#[from] IdentifiersProviderError),

    #[error("agent control remote config validation error: `{0}`")]
    RemoteConfigValidator(#[from] DynamicConfigValidatorError),

    #[error("resource cleaner error: `{0}`")]
    ResourceCleaner(#[from] ResourceCleanerError),

    #[error("updater error: `{0}`")]
    Updater(#[from] UpdaterError),
}
