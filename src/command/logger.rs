use std::{sync::mpsc::Receiver, thread::spawn};

use crate::command::stream::OutputEvent;

use super::{stream::Event, EventLogger};

use log::{debug, kv::ToValue};

// TODO: add configuration filters or additional fields for logging
#[derive(Default)]
pub struct StdEventReceiver {}

impl EventLogger for StdEventReceiver {
    /// fn log outputs the received data using the debug macro, it does not distinguish between
    /// data received from stdout or stderr (newrelic-infra uses stdout while nr-otel-collector
    /// uses stderr)
    fn log(self, rcv: Receiver<Event>) -> std::thread::JoinHandle<()> {
        spawn(move || {
            rcv.iter().for_each(|event| match event.output {
                OutputEvent::Stdout(line) => {
                    debug!(command = event.metadata.values().to_value(); "{}", line);
                }
                OutputEvent::Stderr(line) => {
                    debug!(command = event.metadata.values().to_value();"{}", line)
                }
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{mpsc::channel, Once};

    use log::Log;

    use crate::command::{
        stream::{Event, Metadata, OutputEvent},
        EventLogger,
    };

    use super::StdEventReceiver;

    // mocked implementation of logger to assert key/values and messages
    #[derive(Clone, Debug)]
    struct MockedLogger {
        expected_command_value: String,
        expected_msg: String,
    }

    impl Log for MockedLogger {
        fn enabled(&self, _metadata: &log::Metadata) -> bool {
            true
        }
        fn log(&self, record: &log::Record) {
            assert_eq!(
                self.expected_command_value,
                record
                    .key_values()
                    .get("command".into())
                    .unwrap()
                    .to_string()
            );
            assert_eq!(self.expected_msg, record.args().to_string());
        }

        fn flush(&self) {}
    }

    static INIT_LOGGER: Once = Once::new();
    pub fn init_logger(mocked_logger: Box<dyn Log>) {
        INIT_LOGGER.call_once(|| {
            log::set_boxed_logger(mocked_logger).unwrap();
            log::set_max_level(log::LevelFilter::Debug);
        });
    }

    #[test]
    fn std_logged_data_and_finish() {
        let metadata = "testbin";
        let send_message = "this is a test";
        let mocked_logger = Box::new(MockedLogger {
            expected_command_value: metadata.to_owned(),
            expected_msg: send_message.to_string(),
        });
        init_logger(mocked_logger);

        let logger = StdEventReceiver::default();

        let (tx, rx) = channel();

        let logger_handle = logger.log(rx);

        tx.send(Event::new(
            OutputEvent::Stderr(send_message.to_owned()),
            Metadata::new(metadata),
        ))
        .unwrap();

        drop(tx);
        assert!(logger_handle.join().is_ok());
    }
}
