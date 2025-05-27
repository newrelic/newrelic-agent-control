use crate::opamp::remote_config::hash::Hash;
use crate::values::yaml_config::YAMLConfig;
use serde::{Deserialize, Serialize};

/// The Config represents either a Local or RemoteConfig, being the LocalConfig only a YAMLConfig
/// and the Remote Config including also the hash and status.
#[derive(Debug, PartialEq)]
pub enum Config {
    LocalConfig(LocalConfig),
    RemoteConfig(RemoteConfig),
}

impl Default for Config {
    fn default() -> Self {
        Config::LocalConfig(LocalConfig::default())
    }
}

impl Config {
    pub fn get_yaml_config(&self) -> YAMLConfig {
        match self {
            Config::LocalConfig(local_config) => local_config.0.clone(),
            Config::RemoteConfig(remote_config) => remote_config.config.clone(),
        }
    }

    pub fn get_hash(&self) -> Option<Hash> {
        match self {
            Config::LocalConfig(_) => None,
            Config::RemoteConfig(remote_config) => Some(remote_config.config_hash.clone()),
        }
    }
}

#[derive(Debug, PartialEq, Deserialize, Serialize, Default, Clone)]
pub struct LocalConfig(YAMLConfig);

impl From<YAMLConfig> for LocalConfig {
    fn from(yaml_config: YAMLConfig) -> Self {
        LocalConfig(yaml_config)
    }
}

#[derive(Debug, PartialEq, Deserialize, Serialize, Clone)]
pub struct RemoteConfig {
    pub config: YAMLConfig,
    #[serde(flatten)]
    pub config_hash: Hash,
}

impl RemoteConfig {
    pub fn new(config: YAMLConfig, config_hash: Hash) -> Self {
        Self {
            config,
            config_hash,
        }
    }
}
