pub mod cancellation;
pub mod channel;

/// EVENTS
use crate::opamp::{LastErrorCode, LastErrorMessage};
use crate::sub_agent::health::health_checker::{Healthy, Unhealthy};
use crate::sub_agent::health::with_start_time::{
    HealthWithTimes, HealthyWithTimes, UnhealthyWithTimes,
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
    SubAgentBecameUnhealthy(AgentID, AgentTypeFQN, UnhealthyWithTimes),
    SubAgentBecameHealthy(AgentID, AgentTypeFQN, HealthyWithTimes),
    SubAgentRemoved(AgentID),
    SuperAgentStopped,
    OpAMPConnected,
    OpAMPConnectFailed(Option<LastErrorCode>, LastErrorMessage),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentEvent {
    ConfigUpdated(AgentID),
    SubAgentBecameHealthy(AgentID, HealthyWithTimes),
    SubAgentBecameUnhealthy(AgentID, UnhealthyWithTimes),
}

impl SubAgentEvent {
    pub fn from_health_with_times(health: HealthWithTimes, id: AgentID) -> Self {
        match health {
            HealthWithTimes::Healthy(healthy) => SubAgentEvent::SubAgentBecameHealthy(id, healthy),
            HealthWithTimes::Unhealthy(unhealthy) => {
                SubAgentEvent::SubAgentBecameUnhealthy(id, unhealthy)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentInternalEvent {
    StopRequested,
    AgentBecameUnhealthy(UnhealthyWithTimes),
    AgentBecameHealthy(HealthyWithTimes),
}

impl From<UnhealthyWithTimes> for SubAgentInternalEvent {
    fn from(unhealthy: UnhealthyWithTimes) -> Self {
        SubAgentInternalEvent::AgentBecameUnhealthy(unhealthy)
    }
}

impl From<HealthyWithTimes> for SubAgentInternalEvent {
    fn from(healthy: HealthyWithTimes) -> Self {
        SubAgentInternalEvent::AgentBecameHealthy(healthy)
    }
}

impl From<HealthWithTimes> for SubAgentInternalEvent {
    fn from(health: HealthWithTimes) -> Self {
        match health {
            HealthWithTimes::Healthy(healthy) => healthy.into(),
            HealthWithTimes::Unhealthy(unhealthy) => unhealthy.into(),
        }
    }
}
