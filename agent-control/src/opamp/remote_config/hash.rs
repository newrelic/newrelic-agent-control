use serde::{Deserialize, Serialize};

use super::report::OpampRemoteConfigStatus;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Hash, Eq)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "state")]
pub enum ConfigState {
    Applying,
    Applied,
    Failed { error_message: String },
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone, Hash, Eq)]
pub struct Hash(String);

impl Hash {
    pub fn new<S: AsRef<str>>(hash: S) -> Self {
        Self(hash.as_ref().to_string())
    }

    pub fn get(&self) -> String {
        self.0.clone()
    }
}

impl ConfigState {
    pub fn state(&self) -> ConfigState {
        self.clone()
    }

    pub fn update_state(&mut self, config_state: &ConfigState) {
        *self = config_state.clone()
    }

    pub fn is_applied(&self) -> bool {
        self == &ConfigState::Applied
    }

    pub fn is_applying(&self) -> bool {
        self == &ConfigState::Applying
    }

    pub fn is_failed(&self) -> bool {
        matches!(&self, ConfigState::Failed { .. })
    }

    pub fn error_message(&self) -> Option<String> {
        match &self {
            ConfigState::Failed { error_message: msg } => Some(msg.clone()),
            _ => None,
        }
    }
}

impl From<ConfigState> for OpampRemoteConfigStatus {
    fn from(config_state: ConfigState) -> Self {
        match &config_state {
            ConfigState::Applying => Self::Applying,
            ConfigState::Applied => Self::Applied,
            ConfigState::Failed { error_message } => Self::Error(error_message.to_owned()),
        }
    }
}
