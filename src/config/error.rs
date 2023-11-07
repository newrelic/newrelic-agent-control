use crate::config::store::SuperAgentConfigStoreError;
use std::fmt::Debug;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SuperAgentConfigError {
    #[error("error loading config: `{0}`")]
    LoadConfigError(#[from] SuperAgentConfigStoreError),

    #[error("cannot find config for agent: `{0}`")]
    SubAgentNotFound(String),

    #[error("sub agents configuration not found in the remote config map")]
    SubAgentsNotFound,

    #[error("`{0}`")]
    InvalidYamlConfiguration(#[from] serde_yaml::Error),
}
