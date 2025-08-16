//! This module defines the event system used for communication between components.
//!
//! It provides various event types for different communication patterns: (OpAMP, application lifecycle, internal events...).
//!
//! The module also includes supporting functionality through submodules.
//!
pub mod broadcaster;
pub mod cancellation;
pub mod channel;

use crate::health::with_start_time::HealthWithStartTime;
use crate::opamp::{LastErrorCode, LastErrorMessage};
use crate::sub_agent::identity::AgentIdentity;
use crate::version_checker::AgentVersion;
use crate::{agent_control::agent_id::AgentID, opamp::remote_config::OpampRemoteConfig};
use std::time::SystemTime;

/// Defines the events sent by the OpAMP client.
#[derive(Clone, Debug, PartialEq)]
pub enum OpAMPEvent {
    RemoteConfigReceived(OpampRemoteConfig),
    Connected,
    ConnectFailed(Option<LastErrorCode>, LastErrorMessage),
}

/// Defines application events: these events are sent directly to the application. Eg: OS-signals.
#[derive(Clone, Debug, PartialEq)]
pub enum ApplicationEvent {
    StopRequested,
}

/// Defines the events produced by the AgentControl component.
#[derive(Clone, Debug, PartialEq)]
pub enum AgentControlEvent {
    HealthUpdated(HealthWithStartTime),
    SubAgentRemoved(AgentID),
    AgentControlStopped,
    OpAMPConnected,
    OpAMPConnectFailed(Option<LastErrorCode>, LastErrorMessage),
}

/// Defines the events produced by the SubAgent component.
#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentEvent {
    HealthUpdated(AgentIdentity, HealthWithStartTime),
    SubAgentStarted(AgentIdentity, SystemTime),
}

impl SubAgentEvent {
    pub fn new_health(agent_identity: AgentIdentity, health: HealthWithStartTime) -> Self {
        Self::HealthUpdated(agent_identity, health)
    }
}

/// Defines internal events for the AgentControl component
#[derive(Clone, Debug, PartialEq)]
pub enum AgentControlInternalEvent {
    HealthUpdated(HealthWithStartTime),
    AgentControlCdVersionUpdated(AgentVersion)
}

/// Defines internal events for the SubAgent component.
#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentInternalEvent {
    StopRequested,
    AgentHealthInfo(HealthWithStartTime),
    AgentVersionInfo(AgentVersion),
}
