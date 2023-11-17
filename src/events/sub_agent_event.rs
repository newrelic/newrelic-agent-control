use std::sync::mpsc::{Sender};
use crate::config::super_agent_configs::AgentID;
use crate::events::event::{Event, EventError, EventPublisher};
use crate::events::opamp_event::OpAMPEvent;

const SUB_AGENT_STOPPED_EVENT_NAME:&str = "stopped";

pub enum SubAgentEvent {
    Stopped(AgentID),
}


impl Event for SubAgentEvent {
    fn event_name(&self) -> &str {
        match self {
            SubAgentEvent::Stopped(_) => { SUB_AGENT_STOPPED_EVENT_NAME }
        }
    }
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
    fn publish(&self, event: SubAgentEvent) -> Result<(), EventError> {
        Ok(self.event_sender.send(event)?)
    }
}
