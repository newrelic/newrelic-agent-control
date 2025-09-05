use crate::agent_control::agent_id::{AgentID, SubAgentID};
use crate::agent_control::defaults::{
    AGENT_CONTROL_NAMESPACE, AGENT_CONTROL_TYPE, AGENT_CONTROL_VERSION,
};
use crate::agent_type::agent_type_id::AgentTypeID;
use std::fmt::{Display, Formatter};

pub const ID_ATTRIBUTE_NAME: &str = "id";

#[derive(Clone, Debug, PartialEq)]
pub struct SubAgentIdentity {
    pub id: SubAgentID,
    pub agent_type_id: AgentTypeID,
}

impl From<(SubAgentID, AgentTypeID)> for SubAgentIdentity {
    fn from(value: (SubAgentID, AgentTypeID)) -> Self {
        SubAgentIdentity {
            id: value.0,
            agent_type_id: value.1,
        }
    }
}

impl Display for SubAgentIdentity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.agent_type_id, self.id)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    impl Default for SubAgentIdentity {
        fn default() -> Self {
            SubAgentIdentity {
                id: SubAgentID::try_from("default").unwrap(),
                agent_type_id: AgentTypeID::try_from("default/default:0.0.1").unwrap(),
            }
        }
    }

    #[test]
    fn test_new_agent_control_identity() {
        // Asserts that all fields are correctly set and this doesn't cause a panic
        let _ = SubAgentIdentity::new_agent_control_identity();
    }
}
