pub mod channel;

use crate::opamp::LastErrorMessage;
/// EVENTS
use crate::{opamp::remote_config::RemoteConfig, super_agent::config::AgentID};

#[derive(Clone, Debug, PartialEq)]
pub enum OpAMPEvent {
    RemoteConfigReceived(RemoteConfig),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ApplicationEvent {
    StopRequested,
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
