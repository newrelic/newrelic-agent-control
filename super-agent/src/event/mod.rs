pub mod channel;

use crate::opamp::{LastErrorCode, LastErrorMessage};
use crate::super_agent::config::AgentTypeFQN;
/// EVENTS
use crate::{opamp::remote_config::RemoteConfig, super_agent::config::AgentID};

#[derive(Clone, Debug, PartialEq)]
pub enum OpAMPEvent {
    RemoteConfigReceived(RemoteConfig),
    Connected,
    ConnectFailed(LastErrorCode, LastErrorMessage),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ApplicationEvent {
    StopRequested,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SuperAgentEvent {
    SuperAgentBecameUnhealthy(LastErrorMessage),
    SuperAgentBecameHealthy,
    SubAgentBecameUnhealthy(AgentID, AgentTypeFQN, LastErrorMessage),
    SubAgentBecameHealthy(AgentID, AgentTypeFQN),
    SubAgentRemoved(AgentID),
    SuperAgentStopped,
    OpAMPConnected,
    OpAMPConnectFailed(LastErrorCode, LastErrorMessage),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentEvent {
    ConfigUpdated(AgentID),
    SubAgentBecameHealthy(AgentID),
    SubAgentBecameUnhealthy(AgentID, LastErrorMessage),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentInternalEvent {
    StopRequested,
    AgentBecameUnhealthy(LastErrorMessage),
    AgentBecameHealthy,
}
