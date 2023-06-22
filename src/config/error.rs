use config::ConfigError as ConfigCrateError;
use std::fmt::Debug;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SuperAgentConfigError {
    #[error("error loading config: `{0}`")]
    Load(#[from] ConfigCrateError),
}
