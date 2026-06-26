//! Error types for sub-agent building, runtime, collection management, and stopping.

use super::effective_agents_assembler::EffectiveAgentsAssemblerError;
use crate::event::channel::EventPublisherError;
use crate::opamp::client_builder::OpAMPClientBuilderError;
use crate::values::config_repository::ConfigRepositoryError;
use opamp_client::StartedClientError;
use opamp_client::{ClientError, NotStartedClientError};
use std::time::SystemTimeError;
use thiserror::Error;

/// Errors produced while running a sub-agent.
#[derive(Error, Debug)]
pub enum SubAgentError {
    /// System time could not be read.
    #[error("system time error: {0}")]
    SystemTimeError(#[from] SystemTimeError),
    /// The OpAMP client returned an error.
    #[error("OpAMP client error: {0}")]
    OpampClientError(#[from] ClientError),
    /// The OpAMP client could not be built.
    #[error("OpAMP client error: {0}")]
    OpampClientBuilderError(#[from] OpAMPClientBuilderError),
    /// A started OpAMP client operation failed.
    #[error("started opamp client error: {0}")]
    StartedOpampClientError(#[from] StartedClientError),
    /// A not-started OpAMP client operation failed.
    #[error("not started opamp client error: {0}")]
    NotStartedOpampClientError(#[from] NotStartedClientError),
    /// The effective agent could not be assembled.
    #[error("config assembler error: {0}")]
    ConfigAssemblerError(#[from] EffectiveAgentsAssemblerError),
    /// The configuration repository returned an error.
    #[error("sub agent yaml config repository error: {0}")]
    ConfigRepositoryError(#[from] ConfigRepositoryError),
    /// Publishing an event failed.
    #[error("error publishing event: {0}")]
    EventPublisherError(#[from] EventPublisherError),
}

/// Errors produced while building a sub-agent.
#[derive(Error, Debug)]
pub enum SubAgentBuilderError {
    /// A sub-agent error occurred during building.
    #[error("{0}")]
    SubAgent(#[from] SubAgentError),
    /// The effective agent could not be assembled.
    #[error("config assembler error: {0}")]
    ConfigAssemblerError(#[from] EffectiveAgentsAssemblerError),
    /// The OpAMP client could not be built.
    #[error("OpAMP client error: {0}")]
    OpampClientBuilderError(String),
}

/// Errors produced while managing the sub-agent collection.
#[derive(Error, Debug)]
pub enum SubAgentCollectionError {
    /// A sub-agent error occurred.
    #[error("{0}")]
    SubAgent(#[from] SubAgentError),
    /// No sub-agent with the given id exists in the collection.
    #[error("sub agent {0} not found in the collection")]
    SubAgentNotFound(String),
}

/// Errors produced while stopping a sub-agent.
#[derive(Error, Debug)]
pub enum SubAgentStopError {
    /// The event loop could not be signalled to stop.
    #[error("could not stop the sub agent event loop: {0}")]
    SubAgentEventLoop(#[from] EventPublisherError),
    /// The sub-agent thread could not be joined.
    #[error("failed to join the sub agent thread: {0}")]
    SubAgentJoinHandle(String),
    /// The sub-agent runtime returned an error while stopping.
    #[error("failed to stop the sub agent runtime: {0}")]
    SubAgentRuntimeStop(#[from] SubAgentError),
}
