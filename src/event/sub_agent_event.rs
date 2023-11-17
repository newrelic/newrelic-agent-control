use std::sync::mpsc::{Sender};
use crate::config::super_agent_configs::AgentID;
use crate::event::event::{EventError, EventPublisher};
use crate::event::opamp_event::OpAMPEvent;

const SUB_AGENT_STOPPED_EVENT_NAME:&str = "stopped";

pub enum SubAgentEvent {
    Stopped(AgentID),
}


pub struct SubAgentEventPublisher {
    event_sender: Sender<SubAgentEvent>,
}

impl SubAgentEventPublisher {
    pub fn new(event_sender: Sender<SubAgentEvent>) -> Self {
        Self{
            event_sender
        }
    }
}

impl EventPublisher<SubAgentEvent> for SubAgentEventPublisher {
    // TODO : this error mapping don't thing is correct
    fn publish(&self, event: SubAgentEvent) -> Result<(), EventError> {
        Ok(self.event_sender.send(event)?)
    }
}
