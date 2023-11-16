use crossbeam::channel::{Receiver, Sender};
use tracing::error;
use crate::config::super_agent_configs::AgentID;
use crate::event::event::{Event, EventConsumer, EventHandler, EventPublisher};
struct Invented {}

const SUB_AGENT_STOPPED_EVENT_NAME:&str = "remote_config";

pub(crate) enum SubAgentEvent {
    Stopped(AgentID),
}

impl Event for SubAgentEvent {
    fn event_name(&self) -> &str {
        match self {
            SubAgentEvent::Stopped(_) => { SUB_AGENT_STOPPED_EVENT_NAME }
        }
    }
}

pub struct SubAgentEventHandler {}

impl EventHandler<SubAgentEvent> for SubAgentEventHandler {
    fn handle(&self, event: SubAgentEvent) {
        match event.event_name() {
            SUB_AGENT_STOPPED_EVENT_NAME => self.on_agent_stopped(event),
            unsupported => {
                error!(
                    "backoff type {} not supported, setting default",
                    unsupported
                );
            }
        }
    }
}

impl SubAgentEventHandler {
    fn on_agent_stopped(&self, event: SubAgentEvent) {
        unimplemented!()
    }
}
