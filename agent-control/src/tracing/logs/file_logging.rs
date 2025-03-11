use crate::agent_control::defaults::AGENT_CONTROL_LOG_FILENAME;
use crate::tracing::logs::config::LoggingConfigError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

#[derive(Debug, Deserialize, Serialize, Default, PartialEq, Clone)]
pub(crate) struct FileLoggingConfig {
    pub(crate) enabled: bool,
    // Default value is being set by `ConfigPatcher` right after deserialization.
    pub(crate) path: Option<LogFilePath>,
}

impl FileLoggingConfig {
    pub(super) fn setup(
        self,
        default_dir: PathBuf,
    ) -> Result<Option<(NonBlocking, WorkerGuard)>, LoggingConfigError> {
        if !self.enabled {
            return Ok(None);
        }

        // if path is not specified into the config we fall back to a default path
        let path = self.path.unwrap_or(LogFilePath::new(
            default_dir.clone(),
            PathBuf::from(AGENT_CONTROL_LOG_FILENAME),
        ));
        let file_appender = tracing_appender::rolling::hourly(path.parent, path.file_name);
        Ok(Some(tracing_appender::non_blocking(file_appender)))
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
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
    type Error = LoggingConfigError;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        let parent = value
            .parent()
            .ok_or(LoggingConfigError::InvalidFilePath(
                "file path provided must have a valid parent directory".into(),
            ))?
            .into();
        let file_name = value
            .file_name()
            .ok_or(LoggingConfigError::InvalidFilePath(
                "file path provided must have a valid file name".into(),
            ))?
            .into();
        Ok(Self { parent, file_name })
    }
}
