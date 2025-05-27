use thiserror::Error;

use super::config::AgentControlDynamicConfig;

#[derive(Debug, Error)]
pub enum UpdaterError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("update failed: {0}")]
    UpdateFailed(String),
}

pub trait Updater {
    fn update(&self, config: &AgentControlDynamicConfig) -> Result<(), UpdaterError>;
}

pub struct NoOpUpdater;

impl Updater for NoOpUpdater {
    fn update(&self, _config: &AgentControlDynamicConfig) -> Result<(), UpdaterError> {
        Ok(())
    }
}
