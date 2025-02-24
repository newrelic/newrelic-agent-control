use crate::agent_control::config::{AgentID, AgentTypeFQN, SubAgentConfig};
use crate::agent_type::agent_metadata::AgentMetadata;
use std::fmt::{Display, Formatter};

// This could be SubAgentIdentity
#[derive(Clone, Debug, PartialEq)]
pub struct AgentIdentity {
    id: AgentID,
    fqn: AgentTypeFQN,
}

impl AgentIdentity {
    pub fn new(id: AgentID, fqn: AgentTypeFQN) -> Self {
        Self { id, fqn }
    }

    pub fn id(&self) -> &AgentID {
        &self.id
    }
    pub fn fqn(&self) -> &AgentTypeFQN {
        &self.fqn
    }

    pub fn new_agent_control_identity() -> Self {
        Self::new(
            AgentID::new_agent_control_id(),
            AgentTypeFQN::new_agent_control_fqn(),
        )
    }
}

impl From<(AgentID, SubAgentConfig)> for AgentIdentity {
    fn from(value: (AgentID, SubAgentConfig)) -> Self {
        AgentIdentity::new(value.0, value.1.agent_type)
    }
}

impl From<(&AgentID, &SubAgentConfig)> for AgentIdentity {
    fn from(value: (&AgentID, &SubAgentConfig)) -> Self {
        AgentIdentity::new(value.0.clone(), value.1.agent_type.clone())
    }
}

impl Display for AgentIdentity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.fqn, self.id)
    }
}
