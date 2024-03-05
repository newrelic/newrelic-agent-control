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
