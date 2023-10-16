use crate::config::config_loader::SuperAgentConfigLoaderError;
use std::fmt::Debug;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SuperAgentConfigError {
    #[error("error loading config: `{0}`")]
    LoadConfigError(#[from] SuperAgentConfigLoaderError),
}
