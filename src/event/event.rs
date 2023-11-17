use std::sync::mpsc::{RecvError, SendError};
use thiserror::Error;
use crate::event::opamp_event::{OpAMPEvent, OpAMPEventHandler};
use crate::event::sub_agent_event::SubAgentEvent;

#[derive(Error, Debug)]
pub enum EventError {
    #[error("could not receive event: `{0}`")]
    ReceiveMessageError(#[from] RecvError),
    
    #[error("could not send event: `{0}`")]
    SendOpampMessageError(#[from] SendError<OpAMPEvent>),

    #[error("could not send event: `{0}`")]
    SendSubAgentMessageError(#[from] SendError<SubAgentEvent>),
}


pub(crate) trait Event {
    fn event_name(&self) -> String;
}

pub(crate) trait EventHandler<E: Event> {
    fn handle(&self, event:E);
}

pub(crate) trait EventConsumer<E: Event> {
    type EventHandler: EventHandler<E>;
    fn consume(&self) -> Result<(), EventError>;
}

pub(crate) trait EventPublisher<E: Event> {
    fn publish(&self, event: E) -> Result<(), EventError>;
}
