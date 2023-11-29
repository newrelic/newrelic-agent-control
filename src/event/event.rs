use crate::config::super_agent_configs::AgentID;
use crate::opamp::remote_config::{RemoteConfig, RemoteConfigError};
use std::sync::mpsc::{Receiver, Sender};

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    OpAMPEvent(OpAMPEvent),
    SuperAgentEvent(SuperAgentEvent),
    SubAgentEvent(SubAgentEvent),
}

impl From<OpAMPEvent> for Event {
    fn from(event: OpAMPEvent) -> Self {
        Self::OpAMPEvent(event)
    }
}

impl From<SuperAgentEvent> for Event {
    fn from(event: SuperAgentEvent) -> Self {
        Self::SuperAgentEvent(event)
    }
}

impl From<SubAgentEvent> for Event {
    fn from(event: SubAgentEvent) -> Self {
        Self::SubAgentEvent(event)
    }
}

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

pub(crate) trait EventConsumer<E> {
    fn consume(&self) -> E;
}

pub(crate) trait EventPublisher<E> {
    fn publish(&self, event: E);
}

impl<E> EventPublisher<E> for Sender<E> {
    fn publish(&self, event: E) {
        self.send(event).unwrap()
    }
}

impl<E> EventConsumer<E> for Receiver<E> {
    fn consume(&self) -> E {
        self.recv().unwrap()
    }
}
