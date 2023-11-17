use std::sync::mpsc::{Sender};
use crate::config::super_agent_configs::AgentID;
use crate::event::event::{EventError, EventPublisher};
use crate::event::opamp_event::OpAMPEvent;

const SUB_AGENT_STOPPED_EVENT_NAME:&str = "stopped";

pub(crate) enum SubAgentEvent {
    Stopped(AgentID),
}


pub struct SubAgentEventPublisher {
    event_sender: Sender<OpAMPEvent>,
}

impl EventPublisher<OpAMPEvent> for SubAgentEventPublisher {
    // TODO : this error mapping don't thing is correct
    fn publish(&self, event: OpAMPEvent) -> Result<(), EventError> {
        Ok(self.event_sender.send(event)?)
    }
}
