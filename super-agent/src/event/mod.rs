pub mod channel;

use crate::opamp::{LastErrorCode, LastErrorMessage};
use crate::sub_agent::health::health_checker::{Health, Healthy, Unhealthy};
use crate::super_agent::config::AgentTypeFQN;
/// EVENTS
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
    SubAgentBecameUnhealthy(AgentID, AgentTypeFQN, Unhealthy),
    SubAgentBecameHealthy(AgentID, AgentTypeFQN, Healthy),
    SubAgentRemoved(AgentID),
    SuperAgentStopped,
    OpAMPConnected,
    OpAMPConnectFailed(Option<LastErrorCode>, LastErrorMessage),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentEvent {
    ConfigUpdated(AgentID),
    SubAgentBecameHealthy(AgentID, Healthy),
    SubAgentBecameUnhealthy(AgentID, Unhealthy),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentInternalEvent {
    StopRequested,
    AgentBecameUnhealthy(Unhealthy),
    AgentBecameHealthy(Healthy),
}

impl From<Unhealthy> for SubAgentInternalEvent {
    fn from(unhealthy: Unhealthy) -> Self {
        SubAgentInternalEvent::AgentBecameUnhealthy(unhealthy)
    }
}

impl From<Healthy> for SubAgentInternalEvent {
    fn from(healthy: Healthy) -> Self {
        SubAgentInternalEvent::AgentBecameHealthy(healthy)
    }
}

impl From<Health> for SubAgentInternalEvent {
    fn from(health: Health) -> Self {
        match health {
            Health::Healthy(healthy) => healthy.into(),
            Health::Unhealthy(unhealthy) => unhealthy.into(),
        }
    }
}
