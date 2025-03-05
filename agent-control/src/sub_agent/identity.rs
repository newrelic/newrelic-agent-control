use crate::agent_control::agent_id::AgentID;
use crate::agent_type::agent_type_id::AgentTypeID;
use std::fmt::{Display, Formatter};

// This could be SubAgentIdentity
#[derive(Clone, Debug, PartialEq)]
pub struct AgentIdentity {
    pub id: AgentID,
    pub fqn: AgentTypeID,
}

impl AgentIdentity {
    pub fn new_agent_control_identity() -> Self {
        Self::from((
            AgentID::new_agent_control_id(),
            AgentTypeID::new_agent_control_id(),
        ))
    }
}

impl From<(AgentID, AgentTypeID)> for AgentIdentity {
    fn from(value: (AgentID, AgentTypeID)) -> Self {
        AgentIdentity {
            id: value.0,
            fqn: value.1,
        }
    }
}
impl From<(&AgentID, &AgentTypeID)> for AgentIdentity {
    fn from(value: (&AgentID, &AgentTypeID)) -> Self {
        AgentIdentity {
            id: value.0.clone(),
            fqn: value.1.clone(),
        }
    }
}

impl Display for AgentIdentity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.fqn, self.id)
    }
}
