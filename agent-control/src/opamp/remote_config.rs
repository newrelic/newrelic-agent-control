use crate::agent_control::config::AgentID;
use crate::opamp::remote_config::{hash::Hash, signature::SignatureData};
use opamp_client::opamp::proto::{AgentConfigFile, AgentConfigMap, EffectiveConfig};
use signature::Signatures;
use status_manager::error::ConfigStatusManagerError;
use std::collections::HashMap;
use std::string::FromUtf8Error;
use thiserror::Error;

pub mod hash;
pub mod report;
pub mod signature;
pub mod status;
pub mod status_manager;
pub mod validators;

/// This structure represents the remote configuration that we would retrieve from a server via OpAMP.
/// Contains identifying metadata and the actual configuration values
#[derive(Debug, PartialEq, Clone)]
pub struct RemoteConfig {
    pub agent_id: AgentID,
    pub hash: Hash,
    signatures: Option<Signatures>,
    config_map: Option<ConfigurationMap>,
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum RemoteConfigError {
    #[error("invalid UTF-8 sequence: `{0}`")]
    UTF8(#[from] FromUtf8Error),

    #[error("config hash: `{0}` config error: `{1}`")]
    InvalidConfig(String, String),

    #[error("handling of the remote config status failed: `{0}")]
    Handling(#[from] ConfigStatusManagerError),
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
            signatures: None,
        }
    }
    pub fn with_signature(self, signatures: Signatures) -> Self {
        Self {
            signatures: Some(signatures),
            ..self
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

    // gets the config signature if it exists. It fails if there are multiple signatures.
    pub fn get_unique_signature(&self) -> Result<Option<SignatureData>, RemoteConfigError> {
        if let Some(signatures) = &self.signatures {
            match signatures.len() {
                0 => Err(RemoteConfigError::InvalidConfig(
                    self.hash.get(),
                    "empty signature".to_string(),
                )),
                1 => Ok(Some(
                    signatures
                        .iter()
                        .next()
                        // assumes that the sigunature corresponds to the unique config item
                        .map(|(_, signature)| signature.clone())
                        .ok_or(RemoteConfigError::InvalidConfig(
                            self.hash.get(),
                            "getting unique signature".to_string(),
                        ))?,
                )),
                _ => Err(RemoteConfigError::InvalidConfig(
                    self.hash.get(),
                    "too many signature items".to_string(),
                )),
            }
        } else {
            // Agent control config is not signed
            Ok(None)
        }
    }
}

impl ConfigurationMap {
    pub fn new(config_map: HashMap<String, String>) -> Self {
        Self(config_map)
    }
}

impl TryFrom<AgentConfigMap> for ConfigurationMap {
    type Error = RemoteConfigError;

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
