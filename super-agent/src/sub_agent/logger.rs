use std::thread::JoinHandle;
use std::{sync::mpsc::Receiver, thread::spawn};

use tracing::debug;

/// Stream of outputs, either stdout or stderr
#[derive(Debug)]
pub enum LogOutput {
    Stdout(String),
    Stderr(String),
}

// TODO/N2H: Switch to HashMap so it can use a list of key/values
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Metadata(String);

impl Metadata {
    pub fn new<V>(value: V) -> Self
    where
        V: ToString,
    {
        Metadata(value.to_string())
    }

    pub fn values(self) -> String {
        self.0
    }
}

/// AgentLog with Stream of outputs and metadata
#[derive(Debug)]
pub struct AgentLog {
    pub output: LogOutput,
    pub metadata: Metadata,
}

/// This trait represents the capability of an Event Receiver to log its output.
/// The trait consumes itself as the logging is done in a separate thread,
/// the thread handle is returned.
pub trait EventLogger {
    fn log(self, rcv: Receiver<AgentLog>) -> JoinHandle<()>;
}

// TODO: add configuration filters or additional fields for logging
#[derive(Default)]
pub struct StdEventReceiver {}

impl EventLogger for StdEventReceiver {
    /// fn log outputs the received data using the debug macro, it does not distinguish between
    /// data received from stdout or stderr (newrelic-infra uses stdout while nr-otel-collector
    /// uses stderr)
    fn log(self, rcv: Receiver<AgentLog>) -> std::thread::JoinHandle<()> {
        spawn(move || {
            rcv.iter().for_each(|event| match event.output {
                LogOutput::Stdout(log) => {
                    // For the moment we log all sub-agent logs as info.
                    // We should define a feature to add pattern matching per agent_type in order
                    // so we can emit each log line with its correct type.
                    debug!(command = event.metadata.values(), log)
                }
                LogOutput::Stderr(log) => {
                    debug!(command = event.metadata.values(), log)
                }
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{mpsc::channel, Once};

    use log::Log;

    use super::*;

    // mocked implementation of logger to assert key/values and messages
    #[derive(Clone, Debug)]
    struct MockedLogger {}

    impl Log for MockedLogger {
        fn enabled(&self, _metadata: &log::Metadata) -> bool {
            true
        }
        fn log(&self, _record: &log::Record) {}

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
        let mocked_logger = Box::new(MockedLogger {});
        init_logger(mocked_logger);

        let logger = StdEventReceiver::default();

        let (tx, rx) = channel();

        let logger_handle = logger.log(rx);

        tx.send(AgentLog {
            output: LogOutput::Stderr(send_message.to_owned()),
            metadata: Metadata::new(metadata),
        })
        .unwrap();

        drop(tx);
        assert!(logger_handle.join().is_ok());
    }
}
