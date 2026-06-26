//! This module defines the event system used for communication between components.
//!
//! It provides various event types for different communication patterns: (OpAMP, application lifecycle, internal events...).
//!
//! The module also includes supporting functionality through submodules.
//!
pub mod broadcaster;
pub mod cancellation;
pub mod channel;

use opamp_client::operation::settings::AgentDescription;

use crate::checkers::health::with_start_time::HealthWithStartTime;
use crate::opamp::attributes::UpdatedAttributesMessage;
use crate::opamp::{LastErrorCode, LastErrorMessage};
use crate::sub_agent::identity::AgentIdentity;
use crate::{agent_control::agent_id::AgentID, opamp::remote_config::OpampRemoteConfig};
use std::time::SystemTime;

/// Defines the events sent by the OpAMP client.
#[derive(Clone, Debug, PartialEq)]
pub enum OpAMPEvent {
    /// A remote configuration was received from the OpAMP server.
    RemoteConfigReceived(OpampRemoteConfig),
    /// The OpAMP client established a connection with the server.
    Connected,
    /// The OpAMP client failed to connect, carrying the optional error code and message.
    ConnectFailed(Option<LastErrorCode>, LastErrorMessage),
}

/// Defines application events: these events are sent directly to the application. Eg: OS-signals.
#[derive(Clone, Debug, PartialEq)]
pub enum ApplicationEvent {
    /// Requests the application to stop (e.g. triggered by an OS signal).
    StopRequested,
}

/// Defines the events produced by the AgentControl component.
#[derive(Clone, Debug, PartialEq)]
pub enum AgentControlEvent {
    /// The AgentControl health information was updated.
    HealthUpdated(HealthWithStartTime),
    /// A sub-agent was removed, identified by its `AgentID`.
    SubAgentRemoved(AgentID),
    /// The AgentControl component stopped.
    AgentControlStopped,
    /// The AgentControl agent description was updated.
    AgentDescriptionUpdated(AgentDescription),
    /// The AgentControl OpAMP client connected to the server.
    OpAMPConnected,
    /// The AgentControl OpAMP client failed to connect, carrying the optional error code and message.
    OpAMPConnectFailed(Option<LastErrorCode>, LastErrorMessage),
}

/// Defines the events produced by the SubAgent component.
#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentEvent {
    /// The health information of the identified sub-agent was updated.
    HealthUpdated(AgentIdentity, HealthWithStartTime),
    /// The identified sub-agent started at the given time.
    SubAgentStarted(AgentIdentity, SystemTime),
    /// The agent description of the identified sub-agent was updated.
    AgentDescriptionUpdated(AgentIdentity, AgentDescription),
}

impl SubAgentEvent {
    /// Builds a [`SubAgentEvent::HealthUpdated`] event for the given identity and health.
    pub fn new_health(agent_identity: AgentIdentity, health: HealthWithStartTime) -> Self {
        Self::HealthUpdated(agent_identity, health)
    }
}

/// Defines internal events for the AgentControl component
#[derive(Clone, Debug, PartialEq)]
pub enum AgentControlInternalEvent {
    /// The AgentControl health information was updated.
    HealthUpdated(HealthWithStartTime),
    /// The AgentControl attributes were updated.
    AgentControlAttributesUpdated(UpdatedAttributesMessage),
    /// A restart was requested as part of a self-update.
    SelfUpdateRestartRequested(),
}

/// Defines internal events for the SubAgent component.
#[derive(Clone, Debug, PartialEq)]
pub enum SubAgentInternalEvent {
    /// Requests the sub-agent to stop.
    StopRequested,
    /// Carries updated health information for the sub-agent.
    AgentHealthInfo(HealthWithStartTime),
    /// The sub-agent attributes were updated.
    AgentAttributesUpdated(UpdatedAttributesMessage),
}
