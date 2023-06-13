use log::SetLoggerError;
use thiserror::Error;

use crate::config::error::MetaAgentConfigError;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("logging error: `{0}`")]
    LoggingError(#[from] SetLoggerError),

    #[error("channel is not present in the agent initializer")]
    ChannelExtractError,

    #[error("printed debug info")]
    Debug,

    #[error("could not resolve config: `{0}`")]
    ConfigResolveError(#[from] MetaAgentConfigError),
}
