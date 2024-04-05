use crate::opamp::remote_config_hash::Hash;
use crate::super_agent::config::AgentID;
use opamp_client::opamp::proto::AgentConfigMap;
use std::collections::HashMap;
use std::str::Utf8Error;
use thiserror::Error;

#[derive(Debug, PartialEq, Clone)]
pub struct RemoteConfig {
    pub agent_id: AgentID,
    pub hash: Hash,
    config_map: Option<ConfigMap>,
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum RemoteConfigError {
    #[error("invalid UTF-8 sequence: `{0}`")]
    UTF8(#[from] Utf8Error),

    #[error("config hash: `{0}` config error: `{1}`")]
    InvalidConfig(String, String),
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct ConfigMap(HashMap<String, String>);

impl RemoteConfig {
    pub fn new(agent_id: AgentID, hash: Hash, config_map: Option<ConfigMap>) -> Self {
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

impl ConfigMap {
    pub fn new(config_map: HashMap<String, String>) -> Self {
        Self(config_map)
    }
}

impl TryFrom<&AgentConfigMap> for ConfigMap {
    type Error = RemoteConfigError;

    fn try_from(agent_config_map: &AgentConfigMap) -> Result<Self, Self::Error> {
        agent_config_map.config_map.iter().try_fold(
            ConfigMap::default(),
            |mut result: ConfigMap, (key, value)| {
                let body = std::str::from_utf8(&value.body)?;
                let _ = result.0.insert(key.clone(), body.to_string());
                Ok(result)
            },
        )
    }
}
