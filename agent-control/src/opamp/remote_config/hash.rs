use std::fmt::{self, Display};

use serde::{Deserialize, Serialize};

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

    pub fn error_message(&self) -> Option<&String> {
        match &self {
            ConfigState::Failed { error_message: msg } => Some(msg),
            _ => None,
        }
    }
}

#[cfg(test)]
impl Hash {
    /// Returns the `hash` corresponding to the provided value
    pub(crate) fn new(s: &str) -> Self {
        use std::hash::{Hash as StdHash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        s.to_string().hash(&mut hasher);
        Self::from(hasher.finish().to_string())
    }
}
