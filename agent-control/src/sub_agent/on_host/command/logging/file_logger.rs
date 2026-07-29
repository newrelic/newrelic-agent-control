//! File logging for executable output, using a daily-rotating appender and a thread-local subscriber.

use crate::agent_control::agent_id::AgentID;
use serde::Deserialize;
use std::io::Write;
use std::path::PathBuf;
use thiserror::Error;
use tracing::{level_filters::LevelFilter, subscriber::DefaultGuard};
use tracing_appender::{
    non_blocking::{NonBlocking, WorkerGuard},
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{
    FmtSubscriber,
    fmt::format::{DefaultFields, Format, Full},
};

const MAX_LOG_FILES_SUB_AGENT: usize = 30;

/// Error produced while building a file logger.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct FileLoggerError(String);

#[derive(Debug, Deserialize, Default, PartialEq, Clone)]
pub struct FileLoggingConfigSubAgent {
    pub enabled: bool,
    // Default value is being set by `ConfigPatcher` right after deserialization.
    pub base_path: PathBuf,
}

/// Creates a new file logger writing to a file in the provided directory with the provided suffix.
/// The file will be rotated daily and the file name will be in the format `<timestamp>.<suffix>`
/// e.g. `2027-12-01.stdout.log`. Only the MAX_LOG_FILES_SUB_AGENT most recent rotated files are retained.
pub fn file_logger(
    agent_id: AgentID,
    file_logging_config: FileLoggingConfigSubAgent,
    file_name_suffix: &str,
) -> Result<FileLogger, FileLoggerError> {
    let file_dir = file_logging_config.base_path.join(&agent_id);

    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .max_log_files(MAX_LOG_FILES_SUB_AGENT)
        .filename_suffix(file_name_suffix.to_string())
        .build(file_dir)
        .map_err(|e| FileLoggerError(format!("building file appender: {}", e)))?;

    Ok(FileLogger::new(file_appender))
}

pub(crate) struct FileSystemLoggers {
    out: FileLogger,
    err: FileLogger,
}

impl FileSystemLoggers {
    pub(crate) fn new(out: FileLogger, err: FileLogger) -> Self {
        Self { out, err }
    }

    pub(crate) fn into_loggers(self) -> (FileLogger, FileLogger) {
        (self.out, self.err)
    }
}

/// A tracing subscriber that writes executable output to a rotating file.
pub struct FileLogger {
    file_subscriber: FmtSubscriber<DefaultFields, Format<Full, ()>, LevelFilter, NonBlocking>,
    _guard: WorkerGuard,
}

/// Guard that keeps thread-local file logging active until dropped, then restores the global logger.
pub struct SubAgentLoggerGuard {
    _default_guard: DefaultGuard,
    _worker_guard: WorkerGuard,
}

impl FileLogger {
    /// Creates a file logger writing to the given appender (no ANSI, target, level, or timestamps).
    pub fn new(appender: impl Write + Send + 'static) -> Self {
        let (non_blocking, _guard) = tracing_appender::non_blocking(appender);
        let file_subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_target(false)
            .with_level(false)
            .without_time()
            .with_writer(non_blocking)
            .finish();
        Self {
            file_subscriber,
            _guard,
        }
    }

    /// Enables file logging for the current thread. This disables the global logger defined previously.
    /// To restore the previous global logger, the returned guard must be dropped.
    pub fn set_file_logging(self) -> SubAgentLoggerGuard {
        let default_guard = tracing::subscriber::set_default(self.file_subscriber);
        SubAgentLoggerGuard {
            _default_guard: default_guard,
            _worker_guard: self._guard,
        }
    }
}
