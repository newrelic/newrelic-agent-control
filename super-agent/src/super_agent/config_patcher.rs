use crate::opamp::auth::config::{LocalConfig, ProviderConfig};

use super::config::SuperAgentConfig;

/// Holds the logic to patch the super-agent configuration with any value that
/// cannot be obtained while deserializing.
pub struct ConfigPatcher<'a> {
    local_data_dir: &'a str,
}

impl<'a> ConfigPatcher<'a> {
    pub fn new(local_data_dir: &'a str) -> Self {
        Self { local_data_dir }
    }

    pub fn patch(&self, config: &mut SuperAgentConfig) {
        // Set default value for OpAMP's auth provider using the super-agent local data directory path.
        if let Some(opamp_config) = &mut config.opamp {
            if let Some(auth_config) = &mut opamp_config.auth_config {
                if auth_config.provider.is_none() {
                    auth_config.provider =
                        Some(ProviderConfig::Local(LocalConfig::new(self.local_data_dir)));
                }
            }
        }
    }
}
