use std::fmt::{self, Display};

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

impl<S: AsRef<str>> From<S> for Hash {
    fn from(hash: S) -> Self {
        Self(hash.as_ref().to_string())
    }
}

impl Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ConfigState {
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
