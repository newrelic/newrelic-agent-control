use std::collections::HashMap;
use std::thread::JoinHandle;
use std::{sync::mpsc::Receiver, thread::spawn};

use tracing::debug;

use crate::super_agent::config::AgentID;

/// Stream of outputs, either stdout or stderr
#[derive(Debug)]
pub enum LogOutput {
    Stdout(String),
    Stderr(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    agent_id: AgentID,
    labels: HashMap<String, String>,
}

impl Metadata {
    pub fn new(agent_id: AgentID) -> Self {
        Metadata {
            agent_id,
            labels: HashMap::default(),
        }
    }

    pub fn with_labels<M, K, V>(self, labels: M) -> Self
    where
        M: IntoIterator<Item = (K, V)>,
        K: ToString,
        V: ToString,
    {
        Self {
            labels: labels
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..self
        }
    }

    pub fn get_agent_id(&self) -> &AgentID {
        &self.agent_id
    }

    pub fn get_labels(&self, key: &str) -> Option<&String> {
        self.labels.get(key)
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
                    debug!(agent_id = event.metadata.get_agent_id().to_string(), log)
                }
                LogOutput::Stderr(log) => {
                    debug!(command = event.metadata.get_agent_id().to_string(), log)
                }
            })
        })
    }
}
