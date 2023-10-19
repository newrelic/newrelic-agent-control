use std::collections::HashMap;
use std::str::Utf8Error;
use thiserror::Error;
use crate::config::agent_configs::AgentID;

#[derive(Debug, PartialEq, Clone)]
pub struct RemoteConfig {
    pub agent_id: AgentID,
    pub hash: String,
    pub config_map: HashMap<String, String>,
}

#[derive(Error, Debug, Clone)]
pub enum RemoteConfigError {
    #[error("Invalid UTF-8 sequence: `{0}`")]
    UTF8(#[from] Utf8Error),
}