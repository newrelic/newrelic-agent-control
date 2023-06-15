use std::fmt::Debug;
use thiserror::Error;

use crate::config::error::MetaAgentConfigError;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("channel is not present in the agent initializer")]
    ChannelExtractError,

    #[error("could not resolve config: `{0}`")]
    ConfigResolveError(#[from] MetaAgentConfigError),
}
