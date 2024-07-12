use super::config::LoggingError;
use serde::Deserialize;
use std::path::PathBuf;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

#[derive(Debug, Deserialize, Default, PartialEq, Clone)]
pub(crate) struct FileLoggingConfig {
    pub(crate) enable: bool,
    // Default value is being set by `ConfigPatcher` right after deserialization.
    pub(crate) path: Option<LogFilePath>,
}

impl FileLoggingConfig {
    pub(super) fn setup(self) -> Result<Option<(NonBlocking, WorkerGuard)>, LoggingError> {
        if !self.enable {
            return Ok(None);
        }

        let path = self.path.ok_or(LoggingError::LogFilePathNotDefined)?;
        let file_appender = tracing_appender::rolling::hourly(path.parent, path.file_name);
        Ok(Some(tracing_appender::non_blocking(file_appender)))
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(try_from = "PathBuf")]
pub(crate) struct LogFilePath {
    parent: PathBuf,
    file_name: PathBuf,
}

impl LogFilePath {
    pub fn new(parent: PathBuf, file_name: PathBuf) -> Self {
        Self { parent, file_name }
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
