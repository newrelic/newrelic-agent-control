use std::{sync::mpsc::Receiver, thread::spawn};

use crate::command::stream::OutputEvent;

use super::{stream::Event, EventLogger};

use log::{debug, error, kv::ToValue};

// TODO: add configuration filters or additional fiels for logging
pub struct StdEventReceiver {}

impl Default for StdEventReceiver {
    fn default() -> Self {
        Self {}
    }
}

impl EventLogger for StdEventReceiver {
    fn log(self, rcv: Receiver<Event>) -> std::thread::JoinHandle<()> {
        spawn(move || {
            rcv.iter().for_each(|event| match event.output {
                OutputEvent::Stdout(line) => {
                    debug!(command = event.metadata.values().to_value(); "{}", line);
                }
                OutputEvent::Stderr(line) => {
                    error!(command = event.metadata.values().to_value();"{}", line)
                }
            })
        })
    }
}
