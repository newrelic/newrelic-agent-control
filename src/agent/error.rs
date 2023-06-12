use log::SetLoggerError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("logging error: `{0}`")]
    LoggingError(#[from] SetLoggerError),

    #[error("channel is not present in the agent initializer")]
    ChannelExtractError,

    #[error("printed debug info")]
    Debug,
}
