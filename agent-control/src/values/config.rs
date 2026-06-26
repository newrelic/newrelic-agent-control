//! Configuration value types: the local/remote [`Config`] enum and its variants.

use crate::opamp::remote_config::hash::{ConfigState, Hash};
use crate::values::yaml_config::YAMLConfig;
use serde::{Deserialize, Serialize};

/// The Config represents either a Local or RemoteConfig, being the LocalConfig only a YAMLConfig
/// and the Remote Config including also the hash and status.
#[derive(Debug, PartialEq, Clone)]
pub enum Config {
    /// A configuration sourced locally, wrapping a [`YAMLConfig`].
    LocalConfig(LocalConfig),
    /// A configuration received remotely, including its hash and state.
    RemoteConfig(RemoteConfig),
}

impl Default for Config {
    fn default() -> Self {
        Config::LocalConfig(LocalConfig::default())
    }
}

impl Config {
    /// Returns the underlying [`YAMLConfig`] regardless of the variant.
    pub fn get_yaml_config(&self) -> &YAMLConfig {
        match self {
            Config::LocalConfig(local_config) => &local_config.0,
            Config::RemoteConfig(remote_config) => &remote_config.config,
        }
    }

    /// Returns the config hash for a remote config, or `None` for a local config.
    pub fn get_hash(&self) -> Option<&Hash> {
        match self {
            Config::LocalConfig(_) => None,
            Config::RemoteConfig(remote_config) => Some(&remote_config.hash),
        }
    }

    /// Returns the config state for a remote config, or `None` for a local config.
    pub fn get_state(&self) -> Option<&ConfigState> {
        match self {
            Config::LocalConfig(_) => None,
            Config::RemoteConfig(remote_config) => Some(&remote_config.state),
        }
    }

    /// Returns the [`LocalConfig`] if this is a local config, otherwise `None`.
    pub fn local_config(&self) -> Option<&LocalConfig> {
        match self {
            Config::LocalConfig(local_config) => Some(local_config),
            Config::RemoteConfig(_) => None,
        }
    }

    /// Returns the [`RemoteConfig`] if this is a remote config, otherwise `None`.
    pub fn remote_config(&self) -> Option<&RemoteConfig> {
        match self {
            Config::LocalConfig(_) => None,
            Config::RemoteConfig(remote_config) => Some(remote_config),
        }
    }
}

impl From<Config> for Option<LocalConfig> {
    fn from(value: Config) -> Self {
        match value {
            Config::LocalConfig(local_config) => Some(local_config),
            Config::RemoteConfig(_) => None,
        }
    }
}

impl From<Config> for Option<RemoteConfig> {
    fn from(value: Config) -> Self {
        match value {
            Config::LocalConfig(_) => None,
            Config::RemoteConfig(remote_config) => Some(remote_config),
        }
    }
}

/// A locally-sourced configuration, wrapping a [`YAMLConfig`].
#[derive(Debug, PartialEq, Deserialize, Serialize, Default, Clone)]
pub struct LocalConfig(YAMLConfig);

impl From<YAMLConfig> for LocalConfig {
    fn from(yaml_config: YAMLConfig) -> Self {
        LocalConfig(yaml_config)
    }
}

/// A remotely-received configuration along with its hash and application state.
#[derive(Debug, PartialEq, Deserialize, Serialize, Clone)]
pub struct RemoteConfig {
    /// The configuration values.
    pub config: YAMLConfig,
    /// Hash identifying this remote configuration.
    pub hash: Hash,
    /// Current application state of the configuration.
    #[serde(flatten)]
    pub state: ConfigState,
}

impl RemoteConfig {
    /// Returns true if the configuration has been applied.
    pub fn is_applied(&self) -> bool {
        self.state.is_applied()
    }

    /// Returns true if the configuration is currently being applied.
    pub fn is_applying(&self) -> bool {
        self.state.is_applying()
    }

    /// Returns true if applying the configuration failed.
    pub fn is_failed(&self) -> bool {
        self.state.is_failed()
    }

    /// Returns the configuration with its state replaced by the given one.
    pub fn with_state(self, state: ConfigState) -> Self {
        Self { state, ..self }
    }
}

#[cfg(test)]
#[allow(missing_docs)] // test-support code
pub mod tests {

    use rstest::rstest;
    use serde_json::Value;

    use super::*;

    const EXAMPLE_REMOTE_CONFIG: &str = r#"
    config:
        key: value
    hash: "examplehash"
    state: applying
    "#;

    const EXAMPLE_REMOTE_CONFIG_WITH_ERROR: &str = r#"
    config:
        key: value
    hash: "examplehash"
    state: failed
    error_message: "An error occurred"
    "#;

    impl From<YAMLConfig> for RemoteConfig {
        fn from(yaml_config: YAMLConfig) -> Self {
            RemoteConfig {
                config: yaml_config,
                hash: Hash::default(),
                state: ConfigState::Applying,
            }
        }
    }

    #[rstest]
    #[case(EXAMPLE_REMOTE_CONFIG, RemoteConfig::is_applying, "applying")]
    #[case(EXAMPLE_REMOTE_CONFIG_WITH_ERROR, RemoteConfig::is_failed, "failed")]
    fn basic_serde(
        #[case] example: &str,
        #[case] check_state: impl Fn(&RemoteConfig) -> bool,
        #[case] expected_state: &str,
    ) {
        let remote_config: RemoteConfig = serde_saphyr::from_str(example).unwrap();
        assert_eq!(remote_config.config.get("key").unwrap(), "value");
        assert_eq!(remote_config.hash.to_string(), "examplehash");
        assert!(check_state(&remote_config));

        let serialized_yaml_value = serde_json::to_value(&remote_config).unwrap();
        assert_eq!(serialized_yaml_value["config"]["key"], "value");
        assert_eq!(serialized_yaml_value["hash"], "examplehash");
        assert_eq!(serialized_yaml_value["state"], expected_state);

        let deserialized_yaml_value = serde_saphyr::from_str::<Value>(example).unwrap();
        assert_eq!(deserialized_yaml_value, serialized_yaml_value);
    }
}
