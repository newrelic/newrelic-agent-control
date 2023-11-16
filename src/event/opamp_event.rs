use crossbeam::channel::{Receiver, Sender};
use tracing::error;
use crate::event::event::{Event, EventConsumer, EventHandler, EventPublisher};

struct RemoteConfig {}

struct Invented {}

const REMOTE_CONFIG_EVENT_NAME:&str = "remote_config";
const INVENTED_EVENT_NAME:&str = "invented";

pub(crate) enum OpAMPEvent {
    RemoteConfig(RemoteConfig),
    Invented(Invented)
}

impl Event for OpAMPEvent {
    fn event_name(&self) -> &str {
        match self {
            OpAMPEvent::RemoteConfig(_) => { REMOTE_CONFIG_EVENT_NAME }
            OpAMPEvent::Invented(_) => { INVENTED_EVENT_NAME }
        }
    }
}

pub struct OpAMPEventHandler {}

impl EventHandler<OpAMPEvent> for OpAMPEventHandler {
    fn handle(&self, event: OpAMPEvent) {
        match event.event_name() {
            REMOTE_CONFIG_EVENT_NAME => self.on_remote_config(event),
            INVENTED_EVENT_NAME => self.on_invented(event),
            unsupported => {
                error!(
                    "backoff type {} not supported, setting default",
                    unsupported
                );
            }
        }
    }
}

impl OpAMPEventHandler {
    fn on_remote_config(&self, event: OpAMPEvent) {
        unimplemented!()
    }

    fn on_invented(&self, event: OpAMPEvent) {
        unimplemented!()
    }
}
