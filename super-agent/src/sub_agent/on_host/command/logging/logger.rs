use std::{sync::mpsc::Receiver, thread::JoinHandle};

use super::file_logger::FileLogger;
use crate::super_agent::config::AgentID;
use tracing::{debug, info};

pub(crate) enum Logger {
    File(FileLogger),
    Stdout,
    Stderr,
}

impl Logger {
    pub(crate) fn log<S>(self, rx: Receiver<S>, agent_id: AgentID) -> JoinHandle<()>
    where
        S: ToString + Send + 'static,
    {
        std::thread::spawn(move || {
            match self {
                Self::File(file_logger) => {
                    // If the logger is a FileLogger, set this file logging as the default.
                    // `_guard` needs to exist in scope to keep persisting the logs in the file
                    let _guard = file_logger.set_file_logging();
                    rx.iter()
                        .for_each(|line| info!(%agent_id, "{}", line.to_string()));
                }
                _ => rx
                    .iter()
                    .for_each(|line| debug!(%agent_id, "{}", line.to_string())),
            }
        })
    }
}

impl From<FileLogger> for Logger {
    fn from(file_logger: FileLogger) -> Self {
        Self::File(file_logger)
    }
}
