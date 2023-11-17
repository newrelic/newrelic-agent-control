use std::sync::{Arc, Mutex};
use tracing::error;
use crate::event::event::{Event, EventConsumer, EventError, EventHandler};
use std::sync::mpsc::{Receiver};
use std::thread;
use log::info;

#[derive(Debug)]
pub struct RemoteConfig {
    pub config: String,
    pub hash: String,
}

#[derive(Debug)]
pub struct Invented {
    field: i32,
}

const REMOTE_CONFIG_EVENT_NAME:&str = "remote_config";
const INVENTED_EVENT_NAME:&str = "invented";

#[derive(Debug)]
pub enum OpAMPEvent {
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

pub struct OpAMPEventConsumer {
    event_receiver: Arc<Mutex<Receiver<OpAMPEvent>>>,
    opamp_event_handler: Arc<Mutex<OpAMPEventHandler>>,
}

impl OpAMPEventConsumer {

    pub fn new(event_receiver: Arc<Mutex<Receiver<OpAMPEvent>>>, opamp_event_handler: Arc<Mutex<OpAMPEventHandler>>) -> Self {
        Self{
            event_receiver,
            opamp_event_handler,
        }
    }
}

impl EventConsumer<OpAMPEvent> for OpAMPEventConsumer {
    type EventHandler = OpAMPEventHandler;

    fn consume(&self) {
        let event_receiver = self.event_receiver.clone();
        let opamp_handler = self.opamp_event_handler.clone();
        thread::spawn(move || {
            loop {
                let event = event_receiver.lock().unwrap().recv().unwrap();
                opamp_handler.lock().unwrap().handle(event);
            }
        });
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
    fn on_remote_config(&self, event: OpAMPEvent) {
        info!("{:?}", event)
    }

    fn on_invented(&self, event: OpAMPEvent) {
        unimplemented!()
    }
}
