pub mod cancellation;
pub mod channel;
use crate::opamp::{LastErrorCode, LastErrorMessage};
use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
use crate::sub_agent::identity::AgentIdentity;
use crate::sub_agent::version::version_checker::AgentVersion;
use crate::{agent_control::agent_id::AgentID, opamp::remote_config::RemoteConfig};

#[derive(Clone, Debug, PartialEq)]
pub enum OpAMPEvent {
    RemoteConfigReceived(RemoteConfig),
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
}

impl SubAgentEvent {
    pub fn new(health: HealthWithStartTime, agent_identity: AgentIdentity) -> Self {
        Self::SubAgentHealthInfo(agent_identity, health)
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
