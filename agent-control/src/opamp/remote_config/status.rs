use serde::{Deserialize, Serialize};

use crate::values::yaml_config::YAMLConfig;

use super::{hash::Hash, RemoteConfig, RemoteConfigError};

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct AgentRemoteConfigStatus {
    pub status_hash: Hash,
    pub remote_config: Option<YAMLConfig>,
}

impl TryFrom<RemoteConfig> for AgentRemoteConfigStatus {
    type Error = RemoteConfigError; // FIXME: Review

    fn try_from(value: RemoteConfig) -> Result<Self, Self::Error> {
        if let Some(err) = value.hash.error_message() {
            return Err(RemoteConfigError::InvalidConfig(value.hash.get(), err));
        }

        let values = match value.get_unique()? {
            "" => None,
            config_map => YAMLConfig::try_from(config_map.to_string())
                .map_err(|e| RemoteConfigError::InvalidConfig(value.hash.get(), e.to_string()))?
                .into(),
        };

        Ok(AgentRemoteConfigStatus {
            status_hash: value.hash,
            remote_config: values,
        })
    }
}
