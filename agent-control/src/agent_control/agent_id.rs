use crate::agent_control::defaults::{AGENT_CONTROL_CD_ID, AGENT_CONTROL_ID, RESERVED_AGENT_IDS};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::path::Path;
use thiserror::Error;

const AGENT_ID_MAX_LENGTH: usize = 32;

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Hash, Eq)]
#[serde(try_from = "String")]
#[serde(into = "String")]
/// AgentID is a unique identifier for any agent, including agent-control.
/// It must contain 32 characters at most, contain alphanumeric characters or dashes only,
/// start with alphabetic, and end with alphanumeric,
/// following [RFC 1035 Label names](https://kubernetes.io/docs/concepts/overview/working-with-objects/names/#rfc-1035-label-names).
pub enum AgentID {
    AgentControl,
    K8sCD,
    SubAgent(String),
}

#[derive(Error, Debug)]
pub enum AgentIDError {
    #[error(
        "AgentID must contain 32 characters at most, contain lowercase alphanumeric characters or dashes only, start with alphabetic, and end with alphanumeric"
    )]
    InvalidFormat,
    #[error("AgentID '{0}' is reserved")]
    Reserved(String),
}

impl AgentID {
    pub fn as_str(&self) -> &str {
        match self {
            AgentID::AgentControl => AGENT_CONTROL_ID,
            AgentID::K8sCD => AGENT_CONTROL_CD_ID,
            AgentID::SubAgent(id) => id,
        }
    }

    /// Checks if a string reference has valid format to build an [AgentID].
    /// It follows [RFC 1035 Label names](https://kubernetes.io/docs/concepts/overview/working-with-objects/names/#rfc-1035-label-names),
    /// and sets a shorter maximum length to avoid issues when the agent-id is used to compose names.
    fn is_valid_format(s: &str) -> bool {
        s.len() <= AGENT_ID_MAX_LENGTH
            && s.starts_with(|c: char| c.is_ascii_alphabetic())
            && s.ends_with(|c: char| c.is_ascii_alphanumeric())
            && s.chars()
                .all(|c| c.eq(&'-') || c.is_ascii_digit() || c.is_ascii_lowercase())
    }
}

impl TryFrom<String> for AgentID {
    type Error = AgentIDError;
    fn try_from(str: String) -> Result<Self, Self::Error> {
        if RESERVED_AGENT_IDS.contains(&str.as_str()) {
            Err(AgentIDError::Reserved(str))
        } else if AgentID::is_valid_format(&str) {
            Ok(AgentID::SubAgent(str))
        } else {
            Err(AgentIDError::InvalidFormat)
        }
    }
}

impl TryFrom<&str> for AgentID {
    type Error = AgentIDError;
    fn try_from(str: &str) -> Result<Self, Self::Error> {
        if RESERVED_AGENT_IDS.contains(&str) {
            Err(AgentIDError::Reserved(str.to_string()))
        } else if AgentID::is_valid_format(str) {
            Ok(AgentID::SubAgent(str.to_string()))
        } else {
            Err(AgentIDError::InvalidFormat)
        }
    }
}

impl From<AgentID> for String {
    fn from(val: AgentID) -> Self {
        match val {
            AgentID::AgentControl => AGENT_CONTROL_ID.to_string(),
            AgentID::K8sCD => AGENT_CONTROL_CD_ID.to_string(),
            AgentID::SubAgent(id) => id,
        }
    }
}

impl Display for AgentID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl AsRef<Path> for AgentID {
    fn as_ref(&self) -> &Path {
        // TODO: define how AgentID should be converted to a Path here.
        Path::new(self.as_str())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::agent_control::agent_id::AgentID;
    use crate::agent_control::defaults::AGENT_CONTROL_ID;

    impl Default for AgentID {
        fn default() -> Self {
            AgentID::try_from("default").unwrap()
        }
    }

    #[test]
    fn agent_control_id() {
        let agent_id = AgentID::AgentControl;
        assert_eq!(agent_id.as_str(), AGENT_CONTROL_ID);

        AgentID::try_from(AGENT_CONTROL_ID).unwrap_err();
    }
    #[test]
    fn agent_id_validator() {
        assert!(AgentID::try_from("ab".to_string()).is_ok());
        assert!(AgentID::try_from("a01b".to_string()).is_ok());
        assert!(AgentID::try_from("a-1-b".to_string()).is_ok());
        assert!(AgentID::try_from("a-1".to_string()).is_ok());
        assert!(AgentID::try_from("a".repeat(32)).is_ok());

        assert!(AgentID::try_from("A".to_string()).is_err());
        assert!(AgentID::try_from("1a".to_string()).is_err());
        assert!(AgentID::try_from("a".repeat(33)).is_err());
        assert!(AgentID::try_from("abc012-".to_string()).is_err());
        assert!(AgentID::try_from("-abc012".to_string()).is_err());
        assert!(AgentID::try_from("-".to_string()).is_err());
        assert!(AgentID::try_from("a.b".to_string()).is_err());
        assert!(AgentID::try_from("a*b".to_string()).is_err());
        assert!(AgentID::try_from("abc012/".to_string()).is_err());
        assert!(AgentID::try_from("/abc012".to_string()).is_err());
        assert!(AgentID::try_from("abc/012".to_string()).is_err());
        assert!(AgentID::try_from("aBc012".to_string()).is_err());
        assert!(AgentID::try_from("京bc012".to_string()).is_err());
        assert!(AgentID::try_from("s京123-12".to_string()).is_err());
        assert!(AgentID::try_from("agent-control-①".to_string()).is_err());
    }
}
