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

/// Identifier key for the primary agent configuration within the OpAMP [opamp_client::opamp::proto::AgentConfigMap].
pub const DEFAULT_AGENT_CONFIG_IDENTIFIER: &str = "agentConfig";

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
    #[error("invalid UTF-8 sequence: `{0}`")]
    UTF8(#[from] FromUtf8Error),

    #[error("config hash: `{0}` config error: `{1}`")]
    InvalidConfig(String, String),
}

/// This structure represents the actual configuration values that are stored in the remote config.
#[derive(Debug, Default, PartialEq, Clone)]
pub struct ConfigurationMap(HashMap<String, String>);

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
    pub fn with_signature(self, signatures: Signatures) -> Self {
        Self {
            signatures: Some(signatures),
            ..self
        }
    }

    /// Get configuration value at the
    pub fn get_default(&self) -> Result<&str, OpampRemoteConfigError> {
        self.config_map
            .0
            .get(DEFAULT_AGENT_CONFIG_IDENTIFIER)
            .map(|s| s.as_str())
            .ok_or(OpampRemoteConfigError::InvalidConfig(
                self.hash.to_string(),
                "missing default config".to_string(),
            ))
    }

    /// Get the signature data for the default config
    pub fn get_default_signature(&self) -> Result<Option<SignatureData>, OpampRemoteConfigError> {
        if let Some(signatures) = &self.signatures {
            Ok(Some(
                signatures
                    .signatures
                    .get(DEFAULT_AGENT_CONFIG_IDENTIFIER)
                    .cloned()
                    .ok_or(OpampRemoteConfigError::InvalidConfig(
                        self.hash.to_string(),
                        "missing signature for default config".to_string(),
                    ))?,
            ))
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
