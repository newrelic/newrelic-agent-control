pub mod cancellation;
pub mod channel;

/// EVENTS
use crate::opamp::{LastErrorCode, LastErrorMessage};
use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
use crate::sub_agent::health::with_start_time::{
    HealthWithStartTime, HealthyWithStartTime, UnhealthyWithStartTime,
};
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
    SubAgentBecameUnhealthy(AgentID, AgentTypeFQN, UnhealthyWithStartTime),
    SubAgentBecameHealthy(AgentID, AgentTypeFQN, HealthyWithStartTime),
    SubAgentRemoved(AgentID),
    SuperAgentStopped,
    OpAMPConnected,
    OpAMPConnectFailed(Option<LastErrorCode>, LastErrorMessage),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentEvent {
    ConfigUpdated(AgentID),
    SubAgentBecameHealthy(AgentID, HealthyWithStartTime),
    SubAgentBecameUnhealthy(AgentID, UnhealthyWithStartTime),
}

impl SubAgentEvent {
    pub fn from_health_with_times(health: HealthWithStartTime, id: AgentID) -> Self {
        match health {
            HealthWithStartTime::Healthy(healthy) => {
                SubAgentEvent::SubAgentBecameHealthy(id, healthy)
            }
            HealthWithStartTime::Unhealthy(unhealthy) => {
                SubAgentEvent::SubAgentBecameUnhealthy(id, unhealthy)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentInternalEvent {
    StopRequested,
    AgentBecameUnhealthy(UnhealthyWithStartTime),
    AgentBecameHealthy(HealthyWithStartTime),
}

impl From<UnhealthyWithStartTime> for SubAgentInternalEvent {
    fn from(unhealthy: UnhealthyWithStartTime) -> Self {
        SubAgentInternalEvent::AgentBecameUnhealthy(unhealthy)
    }
}

impl From<HealthyWithStartTime> for SubAgentInternalEvent {
    fn from(healthy: HealthyWithStartTime) -> Self {
        SubAgentInternalEvent::AgentBecameHealthy(healthy)
    }
}

impl From<HealthWithStartTime> for SubAgentInternalEvent {
    fn from(health: HealthWithStartTime) -> Self {
        match health {
            HealthWithStartTime::Healthy(healthy) => healthy.into(),
            HealthWithStartTime::Unhealthy(unhealthy) => unhealthy.into(),
        }
    }
}
