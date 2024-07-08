use crate::opamp::remote_config_hash::Hash;
use crate::super_agent::config::AgentID;
use opamp_client::opamp::proto::{AgentConfigFile, AgentConfigMap, EffectiveConfig};
use std::collections::HashMap;
use std::str::Utf8Error;
use thiserror::Error;

/// This structure represents the remote configuration that we would retrieve from a server via OpAMP.
/// Contains identifying metadata and the actual configuration values
#[derive(Debug, PartialEq, Clone)]
pub struct RemoteConfig {
    pub agent_id: AgentID,
    pub hash: Hash,
    config_map: Option<ConfigurationMap>,
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum RemoteConfigError {
    #[error("invalid UTF-8 sequence: `{0}`")]
    UTF8(#[from] Utf8Error),

    #[error("config hash: `{0}` config error: `{1}`")]
    InvalidConfig(String, String),
}

/// This structure represents the actual configuration values that are stored in the remote config.
#[derive(Debug, Default, PartialEq, Clone)]
pub struct ConfigurationMap(HashMap<String, String>);

impl RemoteConfig {
    pub fn new(agent_id: AgentID, hash: Hash, config_map: Option<ConfigurationMap>) -> Self {
        Self {
            agent_id,
            hash,
            config_map,
        }
    }
    //TODO : This is temporal as when there is only one conf item we should receive an empty string as key
    pub fn get_unique(&self) -> Result<&str, RemoteConfigError> {
        let config_map = self
            .config_map
            .as_ref()
            .ok_or(RemoteConfigError::InvalidConfig(
                self.hash.get(),
                "missing config".to_string(),
            ))?;

        match config_map.0.len() {
            0 => Err(RemoteConfigError::InvalidConfig(
                self.hash.get(),
                "empty config map".to_string(),
            )),
            1 => Ok(config_map
                .0
                .values()
                .next()
                .expect("at least one config has been provided")),
            _ => Err(RemoteConfigError::InvalidConfig(
                self.hash.get(),
                "too many config items".to_string(),
            )),
        }
    }
}

impl ConfigurationMap {
    pub fn new(config_map: HashMap<String, String>) -> Self {
        Self(config_map)
    }
}

impl TryFrom<&AgentConfigMap> for ConfigurationMap {
    type Error = RemoteConfigError;

    fn try_from(agent_config_map: &AgentConfigMap) -> Result<Self, Self::Error> {
        agent_config_map.config_map.iter().try_fold(
            ConfigurationMap::default(),
            |mut result: ConfigurationMap, (key, value)| {
                let body = std::str::from_utf8(&value.body)?;
                let _ = result.0.insert(key.clone(), body.to_string());
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
