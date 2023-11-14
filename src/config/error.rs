use crate::config::store::SuperAgentConfigStoreError;
use crate::opamp::remote_config::RemoteConfigError;
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

    #[error("remote config error: `{0}`")]
    RemoteConfigError(#[from] RemoteConfigError),

    #[error("remote config error: `{0}`")]
    IOError(#[from] std::io::Error),
}
