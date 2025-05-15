use crate::agent_control::agent_id::AgentID;
use crate::agent_control::defaults::{
    AGENT_CONTROL_NAMESPACE, AGENT_CONTROL_TYPE, AGENT_CONTROL_VERSION,
};
use crate::agent_type::agent_type_id::AgentTypeID;
use std::fmt::{Display, Formatter};

pub const ID_ATTRIBUTE_NAME: &str = "id";

// This could be SubAgentIdentity
#[derive(Clone, Debug, PartialEq)]
pub struct AgentIdentity {
    pub id: AgentID,
    pub agent_type_id: AgentTypeID,
}

impl AgentIdentity {
    pub fn new_agent_control_identity() -> Self {
        let ac_agent_type_id = format!(
            "{}/{}:{}",
            AGENT_CONTROL_NAMESPACE, AGENT_CONTROL_TYPE, AGENT_CONTROL_VERSION
        );
        Self::from((
            AgentID::new_agent_control_id(),
            // This is a safe unwrap because we are creating the AgentTypeID from a string that we know is valid.
            // Unit tests will catch any issues with the string format, before it gets to be released.
            AgentTypeID::try_from(ac_agent_type_id.as_str()).unwrap_or_else(|_| {
                panic!("Fail to create AC Agent Type ID from: {ac_agent_type_id}")
            }),
        ))
    }
}

impl From<(AgentID, AgentTypeID)> for AgentIdentity {
    fn from(value: (AgentID, AgentTypeID)) -> Self {
        AgentIdentity {
            id: value.0,
            agent_type_id: value.1,
        }
    }
}
impl From<(&AgentID, &AgentTypeID)> for AgentIdentity {
    fn from(value: (&AgentID, &AgentTypeID)) -> Self {
        AgentIdentity {
            id: value.0.clone(),
            agent_type_id: value.1.clone(),
        }
    }
}

impl Display for AgentIdentity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.agent_type_id, self.id)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    impl Default for AgentIdentity {
        fn default() -> Self {
            AgentIdentity {
                id: AgentID::new("default").unwrap(),
                agent_type_id: AgentTypeID::try_from("default/default:0.0.1").unwrap(),
            }
        }
    }

    #[test]
    fn test_new_agent_control_identity() {
        // Asserts that all fields are correctly set and this doesn't cause a panic
        let _ = AgentIdentity::new_agent_control_identity();
    }
}
