pub mod channel;

use crate::opamp::LastErrorMessage;
/// EVENTS
use crate::{
    opamp::remote_config::{RemoteConfig, RemoteConfigError},
    super_agent::config::AgentID,
};

#[derive(Clone, Debug, PartialEq)]
pub enum OpAMPEvent {
    ValidRemoteConfigReceived(RemoteConfig),
    InvalidRemoteConfigReceived(RemoteConfigError),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SuperAgentEvent {
    StopRequested,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentEvent {
    ConfigUpdated(AgentID),
    SubAgentHealthy(AgentID),
    SubAgentUnhealthy(AgentID, LastErrorMessage),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentInternalEvent {
    StopRequested,
    UnhealthyAgent(LastErrorMessage),
    HealthyAgent,
}
