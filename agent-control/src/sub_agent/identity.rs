use crate::agent_control::config::{AgentID, AgentTypeFQN};
use std::fmt::{Display, Formatter};

// This could be SubAgentIdentity
#[derive(Clone, Debug, PartialEq)]
pub struct AgentIdentity {
    pub id: AgentID,
    pub fqn: AgentTypeFQN,
}

impl AgentIdentity {
    pub fn new_agent_control_identity() -> Self {
        Self::from((
            AgentID::new_agent_control_id(),
            AgentTypeFQN::new_agent_control_fqn(),
        ))
    }
}

impl From<(AgentID, AgentTypeFQN)> for AgentIdentity {
    fn from(value: (AgentID, AgentTypeFQN)) -> Self {
        AgentIdentity {
            id: value.0,
            fqn: value.1,
        }
    }
}
impl From<(&AgentID, &AgentTypeFQN)> for AgentIdentity {
    fn from(value: (&AgentID, &AgentTypeFQN)) -> Self {
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
