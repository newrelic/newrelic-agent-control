pub mod cancellation;
pub mod channel;

/// EVENTS
use crate::opamp::{LastErrorCode, LastErrorMessage};
use crate::sub_agent::health::health_checker::{Health, Healthy, Unhealthy};
use crate::sub_agent::health::with_start_time::{HealthWithStartTime, StartTime};
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
    SubAgentBecameUnhealthy(AgentID, AgentTypeFQN, Unhealthy, StartTime),
    SubAgentBecameHealthy(AgentID, AgentTypeFQN, Healthy, StartTime),
    SubAgentRemoved(AgentID),
    SuperAgentStopped,
    OpAMPConnected,
    OpAMPConnectFailed(Option<LastErrorCode>, LastErrorMessage),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentEvent {
    ConfigUpdated(AgentID),
    SubAgentBecameHealthy(AgentID, Healthy, StartTime),
    SubAgentBecameUnhealthy(AgentID, Unhealthy, StartTime),
}

impl SubAgentEvent {
    pub fn new(health: HealthWithStartTime, id: AgentID) -> Self {
        // We copy the value here
        let start_time = health.start_time();

        match health.into() {
            Health::Healthy(healthy) => {
                SubAgentEvent::SubAgentBecameHealthy(id, healthy, start_time)
            }
            Health::Unhealthy(unhealthy) => {
                SubAgentEvent::SubAgentBecameUnhealthy(id, unhealthy, start_time)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentInternalEvent {
    StopRequested,
    AgentBecameUnhealthy(Unhealthy, StartTime),
    AgentBecameHealthy(Healthy, StartTime),
}

impl From<HealthWithStartTime> for SubAgentInternalEvent {
    fn from(health: HealthWithStartTime) -> Self {
        let start_time = health.start_time();
        match health.into() {
            Health::Healthy(healthy) => Self::AgentBecameHealthy(healthy, start_time),
            Health::Unhealthy(unhealthy) => Self::AgentBecameUnhealthy(unhealthy, start_time),
        }
    }
}
