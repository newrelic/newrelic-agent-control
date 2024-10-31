pub mod cancellation;
pub mod channel;

/// EVENTS
use crate::opamp::{LastErrorCode, LastErrorMessage};
use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
use crate::sub_agent::health::with_start_time::HealthWithStartTime;
use crate::super_agent::config::AgentTypeFQN;
use crate::{opamp::remote_config::RemoteConfig, super_agent::config::AgentID};

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
pub enum SuperAgentEvent {
    SuperAgentBecameUnhealthy(Unhealthy),
    SuperAgentBecameHealthy(Healthy),
    SubAgentRemoved(AgentID),
    SuperAgentStopped,
    OpAMPConnected,
    OpAMPConnectFailed(Option<LastErrorCode>, LastErrorMessage),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentEvent {
    SubAgentHealthInfo(AgentID, AgentTypeFQN, HealthWithStartTime),
}

impl SubAgentEvent {
    pub fn new(health: HealthWithStartTime, id: AgentID, agent_type: AgentTypeFQN) -> Self {
        Self::SubAgentHealthInfo(id, agent_type, health)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentInternalEvent {
    StopRequested,
    AgentHealthInfo(HealthWithStartTime),
}

impl From<HealthWithStartTime> for SubAgentInternalEvent {
    fn from(health: HealthWithStartTime) -> Self {
        Self::AgentHealthInfo(health)
    }
}
