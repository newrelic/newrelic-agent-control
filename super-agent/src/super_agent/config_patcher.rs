use std::path::PathBuf;

use crate::logging::file_logging::LogFilePath;
use crate::opamp::auth::config::{LocalConfig, ProviderConfig};

use super::config::SuperAgentConfig;
use super::defaults::SUPER_AGENT_LOG_FILENAME;

/// Holds the logic to patch the super-agent configuration with any value that
/// cannot be obtained while deserializing.
pub struct ConfigPatcher {
    local_data_dir: PathBuf,
    log_dir: PathBuf,
}

impl ConfigPatcher {
    pub fn new(local_data_dir: PathBuf, log_dir: PathBuf) -> Self {
        Self {
            local_data_dir,
            log_dir,
        }
    }

    pub fn patch(self, config: &mut SuperAgentConfig) {
        // Set default value for OpAMP's auth provider using the super-agent local data directory path.
        if let Some(opamp_config) = &mut config.opamp {
            if let Some(auth_config) = &mut opamp_config.auth_config {
                if auth_config.provider.is_none() {
                    auth_config.provider =
                        Some(ProviderConfig::Local(LocalConfig::new(self.local_data_dir)));
                }
            }
        }

        // Set default value for the log file path using the super-agent log directory path.
        if config.log.file.path.is_none() {
            config.log.file.path = Some(LogFilePath::new(
                self.log_dir,
                PathBuf::from(SUPER_AGENT_LOG_FILENAME),
            ));
        }
    }
}
