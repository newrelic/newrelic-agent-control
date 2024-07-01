use thiserror::Error;

use crate::opamp::remote_config::ConfigurationMap;

/// Error type for the effective configuration loader.
/// This is implementation-dependent so it only encapsulates a string.
#[derive(Debug, Error)]
#[error("error loading effective configuration: `{0}`")]
pub(super) struct LoaderError(String);

trait EffectiveConfigLoader {
    /// Load the effective configuration.
    fn load(&self) -> Result<ConfigurationMap, LoaderError>;
}

struct NoOpEffectiveConfigLoader;

impl EffectiveConfigLoader for NoOpEffectiveConfigLoader {
    fn load(&self) -> Result<ConfigurationMap, LoaderError> {
        Ok(ConfigurationMap::default())
    }
}
