pub mod broadcaster;
pub mod cancellation;
pub mod channel;

use crate::health::health_checker::{Healthy, Unhealthy};
use crate::health::with_start_time::HealthWithStartTime;
use crate::opamp::{LastErrorCode, LastErrorMessage};
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::version::version_checker::AgentVersion;
use crate::{agent_control::agent_id::AgentID, opamp::remote_config::OpampRemoteConfig};
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq)]
pub enum OpAMPEvent {
    RemoteConfigReceived(OpampRemoteConfig),
    Connected,
    ConnectFailed(Option<LastErrorCode>, LastErrorMessage),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ApplicationEvent {
    StopRequested,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AgentControlEvent {
    AgentControlBecameUnhealthy(Unhealthy),
    AgentControlBecameHealthy(Healthy),
    SubAgentRemoved(AgentID),
    AgentControlStopped,
    OpAMPConnected,
    OpAMPConnectFailed(Option<LastErrorCode>, LastErrorMessage),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentEvent {
    SubAgentHealthInfo(AgentIdentity, HealthWithStartTime),
    SubAgentStarted(AgentIdentity, SystemTime),
}

impl SubAgentEvent {
    pub fn new_health(agent_identity: AgentIdentity, health: HealthWithStartTime) -> Self {
        Self::SubAgentHealthInfo(agent_identity, health)
    }

    pub fn new_agent_started(agent_identity: AgentIdentity, started_time: SystemTime) -> Self {
        Self::SubAgentStarted(agent_identity, started_time)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentInternalEvent {
    StopRequested,
    AgentHealthInfo(HealthWithStartTime),
    AgentVersionInfo(AgentVersion),
}

impl From<HealthWithStartTime> for SubAgentInternalEvent {
    fn from(health: HealthWithStartTime) -> Self {
        Self::AgentHealthInfo(health)
    }
}
