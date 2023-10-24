use crate::config::agent_configs::AgentID;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, PartialEq, Clone)]
pub struct RemoteConfig {
    pub agent_id: AgentID,
    pub hash: String,
    pub config_map: HashMap<String, String>,
}

#[derive(Error, Debug, Clone)]
pub enum RemoteConfigError {
    #[error("Config hash: `{0}` Invalid UTF-8 sequence: `{1}`")]
    UTF8(String, String),
}
