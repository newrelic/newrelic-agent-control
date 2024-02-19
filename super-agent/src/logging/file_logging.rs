use std::path::PathBuf;

use serde::Deserialize;

use crate::super_agent::defaults::{SUPER_AGENT_DATA_DIR, SUPER_AGENT_LOG_FILENAME};

use super::config::LoggingError;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
#[derive(Debug, Deserialize, Default, PartialEq, Clone)]
pub(crate) struct FileLoggingConfig {
    enable: bool,
    #[serde(default)]
    file_path: LogFilePath,
}

impl FileLoggingConfig {
    pub(super) fn setup(self) -> Option<(NonBlocking, WorkerGuard)> {
        self.enable.then(|| {
            let file_appender =
                tracing_appender::rolling::hourly(self.file_path.parent, self.file_path.file_name);
            tracing_appender::non_blocking(file_appender)
        })
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(try_from = "PathBuf")]
struct LogFilePath {
    parent: PathBuf,
    file_name: PathBuf,
}

impl Default for LogFilePath {
    fn default() -> Self {
        Self {
            parent: PathBuf::from(SUPER_AGENT_DATA_DIR),
            file_name: PathBuf::from(SUPER_AGENT_LOG_FILENAME),
        }
    }
}

impl TryFrom<PathBuf> for LogFilePath {
    type Error = LoggingError;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        let parent = value
            .parent()
            .ok_or(LoggingError::InvalidFilePath(
                "file path provided must have a valid parent directory".into(),
            ))?
            .into();
        let file_name = value
            .file_name()
            .ok_or(LoggingError::InvalidFilePath(
                "file path provided must have a valid file name".into(),
            ))?
            .into();
        Ok(Self { parent, file_name })
    }
}
