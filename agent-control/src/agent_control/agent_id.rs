use crate::agent_control::defaults::{AGENT_CONTROL_ID, RESERVED_AGENT_IDS};
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
    SubAgent(SubAgentID),
}

impl AgentID {
    pub fn as_str(&self) -> &str {
        match self {
            Self::AgentControl => AGENT_CONTROL_ID,
            Self::SubAgent(id) => id.as_str(),
        }
    }

    /// Checks if a string reference has valid format to build an [AgentID].
    /// It follows [RFC 1035 Label names](https://kubernetes.io/docs/concepts/overview/working-with-objects/names/#rfc-1035-label-names),
    /// and sets a shorter maximum length to avoid issues when the agent-id is used to compose names.
    pub fn is_valid_format(s: &str) -> Result<(), AgentIDError> {
        agent_id_not_reserved_and_valid(s)
    }
}

impl TryFrom<String> for AgentID {
    type Error = AgentIDError;
    fn try_from(input: String) -> Result<Self, Self::Error> {
        agent_id_not_reserved_and_valid(&input)?;
        Ok(Self::SubAgent(SubAgentID::new_unchecked(input)))
    }
}

impl TryFrom<&str> for AgentID {
    type Error = AgentIDError;
    fn try_from(input: &str) -> Result<Self, Self::Error> {
        Self::try_from(input.to_string())
    }
}

impl From<AgentID> for String {
    fn from(val: AgentID) -> Self {
        match val {
            AgentID::AgentControl => AGENT_CONTROL_ID.to_string(),
            AgentID::SubAgent(id) => String::from(id),
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

/// Type with the same API as [`AgentID`], but used to represent only sub-agent IDs.
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Hash, Eq)]
pub struct SubAgentID(String);

impl SubAgentID {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Useful when creating this from an [`AgentID`] input, as the validations were already made.
    fn new_unchecked(s: String) -> Self {
        Self(s)
    }
}

impl TryFrom<String> for SubAgentID {
    type Error = AgentIDError;
    fn try_from(input: String) -> Result<Self, Self::Error> {
        agent_id_not_reserved_and_valid(&input)?;
        Ok(Self(input))
    }
}

impl TryFrom<&str> for SubAgentID {
    type Error = AgentIDError;
    fn try_from(input: &str) -> Result<Self, Self::Error> {
        Self::try_from(input.to_string())
    }
}

impl From<SubAgentID> for String {
    fn from(val: SubAgentID) -> Self {
        val.0
    }
}

impl Display for SubAgentID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl AsRef<Path> for SubAgentID {
    fn as_ref(&self) -> &Path {
        // TODO: define how SubAgentID should be converted to a Path here.
        Path::new(self.as_str())
    }
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

/// Checks if a string reference has valid format to build an [`AgentID`] or [`SubAgentID`].
/// It follows [RFC 1035 Label names](https://kubernetes.io/docs/concepts/overview/working-with-objects/names/#rfc-1035-label-names),
/// and sets a shorter maximum length to avoid issues when the agent-id is used to compose names.
fn agent_id_not_reserved_and_valid(s: impl AsRef<str>) -> Result<(), AgentIDError> {
    let s = s.as_ref();
    if RESERVED_AGENT_IDS
        .iter()
        .any(|id| s.eq_ignore_ascii_case(id))
    {
        Err(AgentIDError::Reserved(s.to_string()))
    } else if agent_id_str_validation(s) {
        Ok(())
    } else {
        Err(AgentIDError::InvalidFormat)
    }
}

fn agent_id_str_validation(s: impl AsRef<str>) -> bool {
    let s = s.as_ref();
    s.len() <= AGENT_ID_MAX_LENGTH
        && s.starts_with(|c: char| c.is_ascii_alphabetic())
        && s.ends_with(|c: char| c.is_ascii_alphanumeric())
        && s.chars()
            .all(|c| c.eq(&'-') || c.is_ascii_digit() || c.is_ascii_lowercase())
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
