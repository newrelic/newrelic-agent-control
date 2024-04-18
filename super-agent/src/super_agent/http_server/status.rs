use serde::Serialize;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use crate::super_agent::config::{AgentID, AgentTypeFQN};

// SuperAgentStatus will contain the information about Super Agent health.
// This information will be shown when the status endpoint is called
// i.e.
// {
//   "super_agent": {
//     "healthy": true,
//     "last_error": "",
//     "status": ""
//   },
// }
#[derive(Debug, Serialize, PartialEq, Default)]
pub struct SuperAgentStatus {
    pub healthy: bool,
    pub last_error: String,
    pub status: String,
}

// OpAMPStatus will contain the information about OpAMP Connection health.
// This information will be shown when the status endpoint is called
// i.e.
// {
//   "opamp": {
//     "enabled": true,
//     "endpoint": "https://example.com/opamp/v1",
//     "reachable": true
//  },
#[derive(Debug, Serialize, PartialEq, Default)]
pub struct OpAMPStatus {
    pub enabled: bool,
    pub endpoint: String,
    pub reachable: bool,
}

// SubAgentStatus will contain the information about all the Sub Agents health.
// This information will be shown when the status endpoint is called
// i.e.
// {
//   "sub_agents": [
//     {
//       "agent_id": "infrastructure_agent_id_1",
//       "agent_type": "newrelic/com.newrelic.infrastructure:0.0.1",
//       "healthy": true,
//       "last_error": "",
//       "status": ""
//     },
//     {
//       "agent_id": "infrastructure_agent_id_1",
//       "agent_type": "newrelic/com.newrelic.infrastructure:0.0.1",
//       "healthy": false,
//       "last_error": "The sub-agent exceeded the number of retries defined in its restart policy.",
//       "status": "[xx/xx/xx xx:xx:xx.xxxx] debug: could not read config at /etc/newrelic-infra.yml"
//     }
//   ]
// }
#[derive(Debug, Serialize, PartialEq, Clone)]
pub(super) struct SubAgentStatus {
    agent_id: AgentID,
    agent_type: AgentTypeFQN,
    healthy: bool,
    last_error: String,
    status: String,
}

impl SubAgentStatus {
    pub fn new(agent_id: AgentID, agent_type: AgentTypeFQN) -> Self {
        Self {
            agent_id,
            agent_type,
            healthy: false,
            last_error: String::default(),
            status: String::default(), // TODO Not implemented yet
        }
    }

    // This struct only has context inside the Sub Agents struct, so it makes it easier to interact
    // if we make it mutable
    pub fn healthy(&mut self) {
        self.healthy = true;
        self.last_error = String::default();
    }

    // This struct only has context inside the Sub Agents struct, so it makes it easier to interact
    // if we make it mutable
    pub fn unhealthy(&mut self, last_error: String) {
        self.healthy = false;
        self.last_error = last_error;
    }
}

#[derive(Debug, PartialEq, Serialize, Default)]
pub(super) struct SubAgentsStatus(HashMap<AgentID, SubAgentStatus>);

impl From<HashMap<AgentID, SubAgentStatus>> for SubAgentsStatus {
    fn from(value: HashMap<AgentID, SubAgentStatus>) -> Self {
        SubAgentsStatus(value)
    }
}

impl SubAgentsStatus {
    pub(super) fn entry(&mut self, agent_id: AgentID) -> Entry<AgentID, SubAgentStatus> {
        self.0.entry(agent_id)
    }

    pub(super) fn remove(&mut self, agent_id: &AgentID) {
        self.0.remove(agent_id);
    }
}

// Status will contain the information about the Super Agent, Sub Agents and OpAMP.
// This information will be shown when the status endpoint is called
// i.e.
// {
//   "super_agent": {
//     "healthy": true,
//     "last_error": "",
//     "status": ""
//   },
//   "opamp": {
//     "enabled": true,
//     "endpoint": "https://example.com/opamp/v1",
//     "reachable": true
//   },
//   "sub_agents": [
//     {
//       "agent_id": "infrastructure_agent_id_1",
//       "agent_type": "newrelic/com.newrelic.infrastructure:0.0.1",
//       "healthy": true,
//       "last_error": "",
//       "status": ""
//     },
//     {
//       "agent_id": "infrastructure_agent_id_1",
//       "agent_type": "newrelic/com.newrelic.infrastructure:0.0.1",
//       "healthy": false,
//       "last_error": "The sub-agent exceeded the number of retries defined in its restart policy.",
//       "status": "[xx/xx/xx xx:xx:xx.xxxx] debug: could not read config at /etc/newrelic-infra.yml"
//     }
//   ]
// }
#[derive(Debug, Serialize, PartialEq, Default)]
pub(super) struct Status {
    pub(super) super_agent: SuperAgentStatus,
    pub(super) opamp: OpAMPStatus,
    pub(super) sub_agents: SubAgentsStatus,
}

#[cfg(test)]
pub mod test {
    use crate::super_agent::config::AgentID;
    use crate::super_agent::http_server::status::{
        Status, SubAgentStatus, SubAgentsStatus, SuperAgentStatus,
    };

    impl Status {
        pub fn with_unhealthy_super_agent(self, error_message: String) -> Self {
            Self {
                super_agent: SuperAgentStatus {
                    healthy: false,
                    last_error: error_message,
                    status: self.super_agent.status,
                },
                ..self
            }
        }

        pub fn with_healthy_super_agent(self) -> Self {
            Self {
                super_agent: SuperAgentStatus {
                    healthy: true,
                    last_error: String::default(),
                    status: self.super_agent.status,
                },
                ..self
            }
        }

        pub fn with_sub_agents(self, sub_agents: SubAgentsStatus) -> Self {
            Self { sub_agents, ..self }
        }
    }

    impl SubAgentsStatus {
        pub fn get(&self, agent_id: &AgentID) -> Option<&SubAgentStatus> {
            self.0.get(agent_id)
        }

        pub fn as_collection(&self) -> Vec<SubAgentStatus> {
            self.0.values().cloned().collect()
        }
    }
}
