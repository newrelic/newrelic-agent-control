use std::{sync::mpsc::Receiver, thread::JoinHandle};

use tracing::{debug, info};

use super::file_logger::FileLogger;

pub(crate) enum Logger {
    File(FileLogger),
    Stdout,
    Stderr,
}

impl Logger {
    pub(crate) fn log<S>(self, rx: Receiver<S>) -> JoinHandle<()>
    where
        S: ToString + Send + 'static,
    {
        std::thread::spawn(move || {
            match self {
                Self::File(file_logger) => {
                    // If the logger is a FileLogger, set this file logging as the default.
                    // `_guard` needs to exist in scope to keep persisting the logs in the file
                    let _guard = file_logger.set_file_logging();
                    rx.iter().for_each(|line| info!("{}", line.to_string()));
                }
                _ => rx.iter().for_each(|line| debug!("{}", line.to_string())),
            }
        })
    }
}

impl From<FileLogger> for Logger {
    fn from(file_logger: FileLogger) -> Self {
        Self::File(file_logger)
    }
}
