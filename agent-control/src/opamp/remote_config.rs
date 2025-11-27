use crate::agent_control::agent_id::AgentID;
use crate::opamp::remote_config::hash::ConfigState;
use crate::opamp::remote_config::{hash::Hash, signature::SignatureData};
use opamp_client::opamp::proto::{AgentConfigFile, AgentConfigMap, EffectiveConfig};
use signature::Signatures;
use std::collections::HashMap;
use std::string::FromUtf8Error;
use thiserror::Error;

pub mod hash;
pub mod report;
pub mod signature;
pub mod validators;

/// Prefix that identifies the agent configuration keys within the OpAMP [opamp_client::opamp::proto::AgentConfigMap].
/// Any key that starts with this prefix is considered part of the agent configuration. See parsing implementation
/// for each case.
pub const AGENT_CONFIG_PREFIX: &str = "agentConfig";

/// This structure represents the remote configuration that we would retrieve from a server via OpAMP.
/// Contains identifying metadata and the actual configuration values
#[derive(Debug, PartialEq, Clone)]
pub struct OpampRemoteConfig {
    pub agent_id: AgentID,
    pub hash: Hash,
    pub state: ConfigState,
    signatures: Option<Signatures>,
    config_map: ConfigurationMap,
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum OpampRemoteConfigError {
    #[error("invalid UTF-8 sequence: {0}")]
    UTF8(#[from] FromUtf8Error),

    #[error("invalid config for hash '{0}': {1}")]
    InvalidConfig(String, String),
}

impl OpampRemoteConfig {
    pub fn new(
        agent_id: AgentID,
        hash: Hash,
        state: ConfigState,
        config_map: ConfigurationMap,
    ) -> Self {
        Self {
            agent_id,
            hash,
            state,
            config_map,
            signatures: None,
        }
    }

    /// Add signature data to the remote config
    pub fn with_signature(self, signatures: Signatures) -> Self {
        Self {
            signatures: Some(signatures),
            ..self
        }
    }

    /// Returns an iterator over the configuration key-value pairs.
    pub fn configs_iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.config_map.0.iter()
    }

    /// Returns an iterator over the configuration key-value pairs that start with [AGENT_CONFIG_PREFIX].
    pub fn agent_configs_iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.config_map
            .0
            .iter()
            .filter(|(k, _)| k.starts_with(AGENT_CONFIG_PREFIX))
    }

    /// Returns true if there are no agent configuration key-value pairs that start with [AGENT_CONFIG_PREFIX]
    /// or all such key-value pairs have empty values.
    pub fn is_agent_configs_empty(&self) -> bool {
        !self
            .config_map
            .0
            .iter()
            .any(|(k, v)| k.starts_with(AGENT_CONFIG_PREFIX) && !v.is_empty())
    }

    /// Get the signature data for a config key
    pub fn signature(&self, config_name: &str) -> Result<SignatureData, OpampRemoteConfigError> {
        let Some(signatures) = &self.signatures else {
            return Err(OpampRemoteConfigError::InvalidConfig(
                self.hash.to_string(),
                "missing signatures".to_string(),
            ));
        };

        signatures
            .signatures
            .get(config_name)
            .cloned()
            .ok_or_else(|| {
                OpampRemoteConfigError::InvalidConfig(
                    self.hash.to_string(),
                    format!("missing signature for config: {}", config_name),
                )
            })
    }
}

/// This structure represents the actual configuration values that are stored in the remote config.
#[derive(Debug, Default, PartialEq, Clone)]
pub struct ConfigurationMap(HashMap<String, String>);

impl ConfigurationMap {
    pub fn new(config_map: HashMap<String, String>) -> Self {
        Self(config_map)
    }
}

impl TryFrom<AgentConfigMap> for ConfigurationMap {
    type Error = OpampRemoteConfigError;

    fn try_from(agent_config_map: AgentConfigMap) -> Result<Self, Self::Error> {
        agent_config_map.config_map.into_iter().try_fold(
            ConfigurationMap::default(),
            |mut result: ConfigurationMap, (key, value)| {
                let body = String::from_utf8(value.body)?;
                let _ = result.0.insert(key, body.to_string());
                Ok(result)
            },
        )
    }
}

impl From<ConfigurationMap> for EffectiveConfig {
    fn from(value: ConfigurationMap) -> Self {
        let config_map = value
            .0
            .into_iter()
            .map(|(k, v)| {
                let agent_config_file = AgentConfigFile {
                    body: v.as_bytes().to_vec(),
                    content_type: "text/yaml".to_string(),
                };
                (k, agent_config_file)
            })
            .collect();

        let config_map = AgentConfigMap { config_map }.into();

        Self { config_map }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::single_agent_config(
        r#"{"agentConfig": "key: value"}"#,
        r#"{"agentConfig": "key: value"}"#
    )]
    #[case::multiple_agent_configs(
        r#"{"agentConfig": "key1: value1", "agentConfig2": "key2: value2"}"#,
        r#"{"agentConfig": "key1: value1", "agentConfig2": "key2: value2"}"#
    )]
    #[case::mixed_configs_filters_non_agent(
        r#"{"agentConfig": "key1: value1", "otherConfig": "key2: value2", "agentConfig3": "key3: value3"}"#,
        r#"{"agentConfig": "key1: value1", "agentConfig3": "key3: value3"}"#
    )]
    #[case::no_agent_configs(
        r#"{"otherConfig": "key1: value1", "someConfig": "key2: value2"}"#,
        r#"{}"#
    )]
    fn test_agent_configs_iter(#[case] config_json: &str, #[case] expected_json: &str) {
        let agent_id = AgentID::try_from("test-agent").unwrap();
        let hash = Hash::from("some-hash");
        let state = ConfigState::Applying;
        let config_map = ConfigurationMap::new(
            serde_json::from_str::<HashMap<String, String>>(config_json).unwrap(),
        );
        let opamp_config = OpampRemoteConfig::new(agent_id, hash, state, config_map);

        let result: HashMap<&String, &String> = opamp_config.agent_configs_iter().collect();
        let expected: HashMap<String, String> = serde_json::from_str(expected_json).unwrap();

        assert_eq!(result.len(), expected.len());
        for (expected_key, expected_value) in &expected {
            assert_eq!(
                result.get(expected_key).map(|v| v.as_str()),
                Some(expected_value.as_str())
            );
        }
    }
}
