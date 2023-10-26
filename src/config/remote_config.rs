use crate::config::remote_config_hash::Hash;
use crate::config::super_agent_configs::AgentID;
use opamp_client::opamp::proto::AgentConfigMap;
use std::collections::HashMap;
use std::str::Utf8Error;
use thiserror::Error;

#[derive(Debug, PartialEq, Clone)]
pub struct RemoteConfig {
    pub agent_id: AgentID,
    pub hash: Hash,
    pub config_map: ConfigMap,
}

#[derive(Error, Debug, Clone)]
pub enum RemoteConfigError {
    #[error("invalid UTF-8 sequence: `{0}`")]
    UTF8(#[from] Utf8Error),

    #[error("config hash: `{0}` config error: `{1}`")]
    InvalidConfig(String, String),
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct ConfigMap(HashMap<String, String>);

impl ConfigMap {
    pub fn new(config_map: HashMap<String, String>) -> Self {
        Self(config_map)
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.0.get(key)
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
