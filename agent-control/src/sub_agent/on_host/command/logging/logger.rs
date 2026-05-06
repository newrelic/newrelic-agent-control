use super::file_logger::FileLogger;
use crate::agent_control::agent_id::AgentID;
use crate::utils::threads::spawn_named_thread;
use crossbeam::channel::Receiver;
use std::thread::JoinHandle;
use tracing::{debug, dispatcher, info};

pub(crate) enum Logger {
    File(Box<FileLogger>, AgentID),
    Stdout(AgentID),
    Stderr(AgentID),
}

impl Logger {
    pub(crate) fn log<S>(self, rx: Receiver<S>) -> JoinHandle<()>
    where
        S: ToString + Send + 'static,
    {
        // We clone the dispatcher so this thread keeps using the same subscriber,
        // but we intentionally do NOT capture/enter the spawning thread's current
        // span: it is long-lived (e.g. `start_agent`), and entering it here would
        // keep it open for this thread's lifetime, causing tracing-opentelemetry to
        // accumulate every emitted event as a span-event in memory without bound.
        let dispatch = dispatcher::get_default(|d| d.clone());

        spawn_named_thread("OnHost logger", move || {
            let _dispatch_guard = dispatcher::set_default(&dispatch);

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
