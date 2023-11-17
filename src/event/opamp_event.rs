use tracing::error;
use crate::event::event::{ConsumerError, Event, EventConsumer, EventError, EventHandler, EventPublisher};
use std::sync::mpsc::{Receiver, Sender};

struct RemoteConfig {
    config: String,
    hash: String,
}

struct Invented {
    field: i32,
}

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

pub(crate) struct OpAMPEventConsumer {
    event_receiver: Receiver<OpAMPEvent>,
    opamp_event_handler: OpAMPEventHandler,
}

impl OpAMPEventConsumer {

    fn new(event_receiver: Receiver<OpAMPEvent>, opamp_event_handler: OpAMPEventHandler) -> Self {
        Self{
            event_receiver,
            opamp_event_handler,
        }
    }
}

impl EventConsumer<OpAMPEvent> for OpAMPEventConsumer {
    type EventHandler = OpAMPEventHandler;

    fn consume(&self) -> Result<(), EventError> {
        loop {
            let event = self.event_receiver.recv()?;
            self.opamp_event_handler.handle(event);
        }
    }
}

pub struct OpAMPEventHandler {}

impl EventHandler<OpAMPEvent> for OpAMPEventHandler {
    fn handle(&self, event:OpAMPEvent) {
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
    fn on_remote_config(&self, event: OpAMPEvent) { unimplemented!() }

    fn on_invented(&self, event: OpAMPEvent) {
        unimplemented!()
    }
}

