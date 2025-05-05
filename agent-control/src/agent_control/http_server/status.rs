use crate::agent_control::agent_id::AgentID;

use crate::agent_type::agent_type_id::AgentTypeID;
use crate::opamp::{LastErrorCode, LastErrorMessage};
use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
use crate::sub_agent::identity::AgentIdentity;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::time::SystemTime;
use url::Url;

/// Agent Control status and health information.
/// This information will be shown when the status endpoint is called.
///
/// Example:
/// ```json
/// {
///   "agent_control": {
///     "healthy": true,
///     "last_error": "",
///     "status": ""
///   },
/// }
/// ```
#[derive(Debug, Serialize, PartialEq, Default, Clone)]
pub struct AgentControlStatus {
    healthy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    status: String,
}

impl AgentControlStatus {
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

/// OpAMP Connection health information.
/// This information will be shown when the status endpoint is called.
///
/// Example:
/// ```json
/// {
///   "fleet": {
///     "enabled": true,
///     "endpoint": "https://example.com/opamp/v1",
///     "reachable": true,
///     "error_code": 403, // present only if reachable == false
///     "error_message": "this is an error message", // present only if reachable == false
///   }
/// }
/// ```
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

/// Sub Agent status and health information.
/// This information is displayed when the status endpoint is called.
///
/// Example:
/// ```json
/// {
///   "sub_agents": [
///     {
///       "agent_id": "infrastructure_agent_id_1",
///       "agent_type": "newrelic/com.newrelic.infrastructure:0.0.1",
///       "health_info": {
///         "healthy": true,
///         "last_error": null,
///         "status": "",
///         "start_time_unix_nano": 0,
///         "status_time_unix_nano": 0
///       },
///       "agent_start_time_unix_nano": 0
///     },
///     {
///       "agent_id": "infrastructure_agent_id_1",
///       "agent_type": "newrelic/com.newrelic.infrastructure:0.0.1",
///       "health_info": {
///         "healthy": false,
///         "last_error": "The sub-agent exceeded the number of retries defined in its restart policy.",
///         "status": "[xx/xx/xx xx:xx:xx.xxxx] debug: could not read config at /etc/newrelic-infra.yml",
///         "start_time_unix_nano": 0,
///         "status_time_unix_nano": 0
///       },
///       "agent_start_time_unix_nano": 0
///     }
///   ]
/// }
/// ```
///
/// Fields:
/// - `agent_id`: The unique identifier of the Sub Agent.
/// - `agent_type`: The type of the Sub Agent, represented as a fully qualified name (FQN).
/// - `agent_start_time_unix_nano`: A `u64` representing the start time of the Sub Agent in nanoseconds since the Unix epoch.
/// - `health_info`: A `HealthInfo` struct containing the health-related information of the Sub Agent.
#[derive(Debug, Serialize, PartialEq, Clone)]
pub(super) struct SubAgentStatus {
    agent_id: AgentID,
    #[serde(serialize_with = "AgentTypeID::serialize_fqn")]
    agent_type: AgentTypeID,
    agent_start_time_unix_nano: u64,
    health_info: HealthInfo,
}

/// Health-related information of a Sub Agent.
/// This struct is used to represent the health status of a Sub Agent
/// and is displayed when the status endpoint is called.
///
/// Example:
/// ```json
/// {
///   "healthy": true,
///   "last_error": null,
///   "status": "Running",
///   "start_time_unix_nano": 1672531200000000000,
///   "status_time_unix_nano": 1672531205000000000
/// }
/// ```
///
/// Fields:
/// - `healthy`: A boolean indicating whether the Sub Agent is healthy.
/// - `last_error`: An optional string containing the last error message, if any.
/// - `status`: A string representing the current status of the Sub Agent.
/// - `start_time_unix_nano`: A `u64` representing the start time of the agent in nanoseconds since the Unix epoch.
/// - `status_time_unix_nano`: A `u64` representing the last status update time in nanoseconds since the Unix epoch.
#[derive(Debug, Serialize, PartialEq, Clone)]
pub(super) struct HealthInfo {
    healthy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    status: String,
    start_time_unix_nano: u64,
    status_time_unix_nano: u64,
}

impl SubAgentStatus {
    pub fn with_identity(agent_identity: AgentIdentity) -> Self {
        Self {
            agent_id: agent_identity.id,
            agent_type: agent_identity.agent_type_id,
            agent_start_time_unix_nano: 0,
            health_info: HealthInfo {
                healthy: false,
                last_error: None,
                status: String::default(),
                start_time_unix_nano: 0,
                status_time_unix_nano: 0,
            },
        }
    }

    pub fn with_start_time(self, start_time: SystemTime) -> Self {
        Self {
            agent_start_time_unix_nano: time_to_unix_timestamp(start_time),
            ..self
        }
    }

    // This struct only has context inside the Sub Agents struct, so it makes it easier to interact
    // if we make it mutable
    pub fn update_health(&mut self, health: HealthWithStartTime) {
        self.health_info.healthy = health.is_healthy();
        self.health_info.last_error = health.last_error();
        self.health_info.status = health.status().to_string();
        self.health_info.start_time_unix_nano = time_to_unix_timestamp(health.start_time());
        self.health_info.status_time_unix_nano = time_to_unix_timestamp(health.status_time());
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

/// Agent Control, Sub Agents and OpAMP status and health.
/// This information will be shown when the status endpoint is called.
///
/// Example:
/// ```json
/// {
///   "agent_control": {
///     "healthy": true,
///     "last_error": "",
///     "status": ""
///   },
///   "fleet": {
///     "enabled": true,
///     "endpoint": "https://example.com/opamp/v1",
///     "reachable": true
///   },
///   "sub_agents": [
///     {
///       "agent_id": "infrastructure_agent_id_1",
///       "agent_type": "newrelic/com.newrelic.infrastructure:0.0.1",
///       "healthy": true,
///       "last_error": "",
///       "status": ""
///     },
///     {
///       "agent_id": "infrastructure_agent_id_1",
///       "agent_type": "newrelic/com.newrelic.infrastructure:0.0.1",
///       "healthy": false,
///       "last_error": "The sub-agent exceeded the number of retries defined in its restart policy.",
///       "status": "[xx/xx/xx xx:xx:xx.xxxx] debug: could not read config at /etc/newrelic-infra.yml"
///     }
///   ]
/// }
/// ```
#[derive(Debug, Serialize, PartialEq, Default)]
pub(super) struct Status {
    pub(super) agent_control: AgentControlStatus,
    pub(super) fleet: OpAMPStatus,
    pub(super) sub_agents: SubAgentsStatus,
}

impl Status {
    pub fn with_opamp(mut self, endpoint: Url) -> Self {
        self.fleet.enabled = true;
        self.fleet.endpoint = Some(endpoint);
        self
    }
}

fn time_to_unix_timestamp(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

#[cfg(test)]
pub mod tests {
    use url::Url;

    use crate::agent_control::agent_id::AgentID;

    use crate::agent_control::http_server::status::{
        AgentControlStatus, HealthInfo, OpAMPStatus, Status, SubAgentStatus, SubAgentsStatus,
    };
    use crate::agent_type::agent_type_id::AgentTypeID;
    use crate::opamp::{LastErrorCode, LastErrorMessage};

    impl Status {
        pub fn with_sub_agents(self, sub_agents: SubAgentsStatus) -> Self {
            Self { sub_agents, ..self }
        }
    }

    impl AgentControlStatus {
        pub fn new_healthy(status: String) -> Self {
            AgentControlStatus {
                healthy: true,
                last_error: None,
                status,
            }
        }
        pub fn new_unhealthy(status: String, last_error: String) -> Self {
            AgentControlStatus {
                healthy: false,
                last_error: Some(last_error),
                status,
            }
        }
    }

    impl SubAgentStatus {
        pub fn new(
            agent_id: AgentID,
            agent_type: AgentTypeID,
            agent_start_time_unix_nano: u64,
            health_info: HealthInfo,
        ) -> Self {
            SubAgentStatus {
                agent_id,
                agent_type,
                agent_start_time_unix_nano,
                health_info,
            }
        }

        pub fn agent_id(&self) -> AgentID {
            self.agent_id.clone()
        }
    }

    impl HealthInfo {
        pub fn new(
            status: String,
            healthy: bool,
            last_error: Option<String>,
            start_time_unix_nano: u64,
            status_time_unix_nano: u64,
        ) -> Self {
            HealthInfo {
                status,
                healthy,
                last_error,
                start_time_unix_nano,
                status_time_unix_nano,
            }
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
