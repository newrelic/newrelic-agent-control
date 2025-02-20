use super::effective_agents_assembler::EffectiveAgentsAssemblerError;
use crate::event::channel::EventPublisherError;
use crate::opamp::client_builder::OpAMPClientBuilderError;
use crate::opamp::hash_repository::repository::HashRepositoryError;
use crate::opamp::remote_config::validators::regexes::ConfigValidatorError;
use crate::opamp::remote_config::validators::signature::validator::SignatureValidatorError;
use crate::opamp::remote_config::RemoteConfigError;
use crate::values::yaml_config::YAMLConfigError;
use crate::values::yaml_config_repository::YAMLConfigRepositoryError;
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
    #[error("remote config hash error: `{0}`")]
    RemoteConfigHashError(#[from] HashRepositoryError),

    #[error("config assembler error: `{0}`")]
    ConfigAssemblerError(#[from] EffectiveAgentsAssemblerError),
    #[error("sub agent yaml config repository error: `{0}`")]
    YAMLConfigRepositoryError(#[from] YAMLConfigRepositoryError),
    #[error("sub agent values error: `{0}`")]
    ValuesUnserializeError(#[from] YAMLConfigError),
    #[error("remote config error: `{0}`")]
    RemoteConfigError(#[from] RemoteConfigError),
    #[error("Error publishing event: `{0}`")]
    EventPublisherError(#[from] EventPublisherError),
    #[error("ConfigValidator error: `{0}`")]
    ConfigValidatorError(#[from] ConfigValidatorError),
    #[error("SignatureValidator error: `{0}`")]
    SignatureValidatorError(#[from] SignatureValidatorError),
}

#[derive(Error, Debug)]
pub enum SubAgentBuilderError {
    #[error("`{0}`")]
    SubAgent(#[from] SubAgentError),
    #[error("config assembler error: `{0}`")]
    ConfigAssemblerError(#[from] EffectiveAgentsAssemblerError),
    #[error("remote config hash error: `{0}`")]
    RemoteConfigHashError(#[from] HashRepositoryError),
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
