use crate::opamp::{LastErrorCode, LastErrorMessage};
use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
use crate::super_agent::config::{AgentID, AgentTypeFQN};
use serde::Serialize;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::time::SystemTime;
use url::Url;

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
#[derive(Debug, Serialize, PartialEq, Default, Clone)]
pub struct SuperAgentStatus {
    healthy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    status: String,
}

impl SuperAgentStatus {
    pub fn healthy(&mut self, healthy: Healthy) {
        self.healthy = true;
        self.last_error = None;
        self.status = healthy.status().to_string();
    }

    pub fn unhealthy(&mut self, unhealthy: Unhealthy) {
        self.healthy = false;
        self.last_error = unhealthy.last_error().to_string().into();
        self.status = unhealthy.status().to_string();
    }
}

// OpAMPStatus will contain the information about OpAMP Connection health.
// This information will be shown when the status endpoint is called
// i.e.
// {
//   "opamp": {
//     "enabled": true,
//     "endpoint": "https://example.com/opamp/v1",
//     "reachable": true,
//     "error_code": 403, // present only if reachable == false
//     "error_message": "this is an error message", // present only if reachable == false
//  },
#[derive(Debug, Serialize, PartialEq, Default, Clone)]
pub struct OpAMPStatus {
    enabled: bool,
    endpoint: Option<Url>,
    reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<LastErrorCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_message: Option<LastErrorMessage>,
}

impl OpAMPStatus {
    pub(super) fn reachable(&mut self) {
        self.reachable = true;
        self.error_code = None;
        self.error_message = None;
    }

    pub(super) fn unreachable(
        &mut self,
        error_code: Option<LastErrorCode>,
        error_message: LastErrorMessage,
    ) {
        self.reachable = false;
        self.error_code = error_code;
        self.error_message = Some(error_message);
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    status: String,
    start_time_unix_nano: u64,
    status_time_unix_nano: u64,
}

impl SubAgentStatus {
    pub fn with_id_and_type(agent_id: AgentID, agent_type: AgentTypeFQN) -> Self {
        Self {
            agent_id,
            agent_type,
            healthy: false,
            last_error: None,
            status: String::default(),
            start_time_unix_nano: 0,
            status_time_unix_nano: 0,
        }
    }

    // This struct only has context inside the Sub Agents struct, so it makes it easier to interact
    // if we make it mutable
    pub fn update_health(&mut self, health: HealthWithStartTime) {
        self.healthy = health.is_healthy();
        self.last_error = health.last_error().map(String::from);
        self.status = health.status().to_string();
        self.start_time_unix_nano = health
            .start_time()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        self.status_time_unix_nano = health
            .status_time()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
    }
}

#[derive(Debug, PartialEq, Serialize, Default, Clone)]
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

impl Status {
    pub fn with_opamp(mut self, endpoint: Url) -> Self {
        self.opamp.enabled = true;
        self.opamp.endpoint = Some(endpoint);
        self
    }
}

#[cfg(test)]
pub mod tests {
    use url::Url;

    use crate::opamp::{LastErrorCode, LastErrorMessage};
    use crate::super_agent::config::{AgentID, AgentTypeFQN};
    use crate::super_agent::http_server::status::{
        OpAMPStatus, Status, SubAgentStatus, SubAgentsStatus, SuperAgentStatus,
    };

    impl Status {
        pub fn with_sub_agents(self, sub_agents: SubAgentsStatus) -> Self {
            Self { sub_agents, ..self }
        }
    }

    impl SuperAgentStatus {
        pub fn new_healthy(status: String) -> Self {
            SuperAgentStatus {
                healthy: true,
                last_error: None,
                status,
            }
        }
        pub fn new_unhealthy(status: String, last_error: String) -> Self {
            SuperAgentStatus {
                healthy: false,
                last_error: Some(last_error),
                status,
            }
        }
    }

    impl SubAgentStatus {
        pub fn new(
            agent_id: AgentID,
            agent_type: AgentTypeFQN,
            status: String,
            healthy: bool,
            last_error: Option<String>,
            start_time_unix_nano: u64,
            status_time_unix_nano: u64,
        ) -> Self {
            SubAgentStatus {
                agent_id,
                agent_type,
                status,
                healthy,
                last_error,
                start_time_unix_nano,
                status_time_unix_nano,
            }
        }

        pub fn agent_id(&self) -> AgentID {
            self.agent_id.clone()
        }
    }

    impl OpAMPStatus {
        pub fn new(
            enabled: bool,
            endpoint: Option<Url>,
            reachable: bool,
            error_code: Option<LastErrorCode>,
            error_message: Option<LastErrorMessage>,
        ) -> Self {
            OpAMPStatus {
                enabled,
                endpoint,
                reachable,
                error_code,
                error_message,
            }
        }

        pub fn enabled_and_reachable(endpoint: Option<Url>) -> Self {
            OpAMPStatus {
                enabled: true,
                endpoint,
                reachable: true,
                error_code: None,
                error_message: None,
            }
        }
        pub fn enabled_and_unreachable(
            endpoint: Option<Url>,
            error_code: LastErrorCode,
            error_message: LastErrorMessage,
        ) -> Self {
            OpAMPStatus {
                enabled: true,
                endpoint,
                reachable: false,
                error_code: Some(error_code),
                error_message: Some(error_message),
            }
        }
    }
}
