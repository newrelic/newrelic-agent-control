use super::file_logger::FileLogger;
use crate::agent_control::config::AgentID;
use crate::utils::threads::spawn_named_thread;
use std::{sync::mpsc::Receiver, thread::JoinHandle};
use tracing::{debug, info};

pub(crate) enum Logger {
    File(FileLogger, AgentID),
    Stdout(AgentID),
    Stderr(AgentID),
}

impl Logger {
    pub(crate) fn log<S>(self, rx: Receiver<S>) -> JoinHandle<()>
    where
        S: ToString + Send + 'static,
    {
        spawn_named_thread("OnHost logger", move || {
            match self {
                Self::File(file_logger, agent_id) => {
                    // If the logger is a FileLogger, set this file logging as the default.
                    // `_guard` needs to exist in scope to keep persisting the logs in the file
                    let _guard = file_logger.set_file_logging();
                    rx.iter()
                        .for_each(|line| info!(%agent_id, "{}", line.to_string()));
                }
                Self::Stderr(agent_id) => {
                    rx.iter()
                        .for_each(|line| debug!(%agent_id, "{}", line.to_string()));
                }
                Self::Stdout(agent_id) => {
                    rx.iter()
                        .for_each(|line| debug!(%agent_id, "{}", line.to_string()));
                }
            }
        })
    }
}
