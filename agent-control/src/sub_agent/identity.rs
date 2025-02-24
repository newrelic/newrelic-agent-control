use crate::agent_control::config::{AgentID, AgentTypeFQN, SubAgentConfig};

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
