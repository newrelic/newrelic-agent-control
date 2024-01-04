pub mod channel;

/// EVENTS
use crate::config::super_agent_configs::AgentID;
use crate::opamp::remote_config::{RemoteConfig, RemoteConfigError};

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
}
