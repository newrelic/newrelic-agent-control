use crate::opamp::remote_config::hash::{ConfigState, Hash};
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
    pub fn get_yaml_config(&self) -> &YAMLConfig {
        match self {
            Config::LocalConfig(local_config) => &local_config.0,
            Config::RemoteConfig(remote_config) => &remote_config.config,
        }
    }

    pub fn get_hash(&self) -> Option<Hash> {
        match self {
            Config::LocalConfig(_) => None,
            Config::RemoteConfig(remote_config) => Some(remote_config.hash()),
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
    config_hash: Hash,
}

impl RemoteConfig {
    pub fn new(config: YAMLConfig, config_hash: Hash) -> Self {
        Self {
            config,
            config_hash,
        }
    }

    pub fn is_applied(&self) -> bool {
        self.config_hash.is_applied()
    }

    pub fn is_applying(&self) -> bool {
        self.config_hash.is_applying()
    }

    pub fn hash(&self) -> Hash {
        self.config_hash.clone()
    }

    pub fn update_state(&mut self, config_state: &ConfigState) {
        self.config_hash.update_state(config_state)
    }
}

#[cfg(test)]
mod tests {

    use serde_yaml::Value;

    use super::*;

    const EXAMPLE_REMOTE_CONFIG: &str = r#"
    config:
        key: value
    hash: "examplehash"
    state: applying
    "#;

    #[test]
    fn basic_serde() {
        let remote_config: RemoteConfig = serde_yaml::from_str(EXAMPLE_REMOTE_CONFIG).unwrap();
        assert_eq!(remote_config.config.get("key").unwrap(), "value");
        assert_eq!(remote_config.hash().get(), "examplehash");
        assert!(remote_config.is_applying());

        let serialized_yaml_value = serde_yaml::to_value(&remote_config).unwrap();
        assert_eq!(serialized_yaml_value["config"]["key"], "value");
        assert_eq!(serialized_yaml_value["hash"], "examplehash");
        assert_eq!(serialized_yaml_value["state"], "applying");

        let deserialized_yaml_value = serde_yaml::from_str::<Value>(EXAMPLE_REMOTE_CONFIG).unwrap();
        assert_eq!(deserialized_yaml_value, serialized_yaml_value);
    }
}
