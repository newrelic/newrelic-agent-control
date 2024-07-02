use thiserror::Error;

use crate::opamp::remote_config::ConfigurationMap;

/// Error type for the effective configuration loader.
/// This is implementation-dependent so it only encapsulates a string.
#[derive(Debug, Error)]
#[error("error loading effective configuration: `{0}`")]
pub struct LoaderError(String);

/// Trait for effective configuration loaders.
pub trait EffectiveConfigLoader {
    /// Load the effective configuration.
    fn load(&self) -> Result<ConfigurationMap, LoaderError>;
}

/// Builder for effective configuration loaders. Currently only supports the no-op loader.
pub struct EffectiveConfigLoaderBuilder;

impl EffectiveConfigLoaderBuilder {
    /// Build a new effective configuration loader.
    pub fn build(&self) -> NoOpEffectiveConfigLoader {
        NoOpEffectiveConfigLoader
    }
}

/// A no-op effective configuration loader that always returns an empty configuration.
pub struct NoOpEffectiveConfigLoader;

/// Implementation of the `EffectiveConfigLoader` trait for the no-op loader. Returns an empty configuration.
impl EffectiveConfigLoader for NoOpEffectiveConfigLoader {
    fn load(&self) -> Result<ConfigurationMap, LoaderError> {
        Ok(ConfigurationMap::default())
    }
}

#[cfg(test)]
pub mod tests {
    use mockall::mock;

    use super::*;

    mock!(
        pub EffectiveConfigLoader {}

        impl EffectiveConfigLoader for EffectiveConfigLoader {
            fn load(&self) -> Result<ConfigurationMap, LoaderError>;
        }
    );

    #[test]
    fn no_op_loader() {
        let loader = EffectiveConfigLoaderBuilder.build();
        let config = loader.load().unwrap();
        assert_eq!(config, ConfigurationMap::default());
    }
}
