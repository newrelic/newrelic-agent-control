use std::sync::mpsc::Receiver;

use crate::command::stream::OutputEvent;

use super::{stream::Event, EventLogger};

use log::{debug, error, kv::ToValue};

pub struct EventReceiver {
    rx: Receiver<Event>,
}

impl EventReceiver {
    pub fn new(rx: Receiver<Event>) -> Self {
        Self { rx }
    }
}

impl EventLogger for EventReceiver {
    fn log(self) {
        // Get any outputs
        self.rx.iter().for_each(|event| match event.output {
            OutputEvent::Stdout(line) => {
                debug!(command = event.metadata.values().to_value(); "{}", line);
            }
            OutputEvent::Stderr(line) => {
                error!(command = event.metadata.values().to_value();"{}", line)
            }
        })
    }
}
