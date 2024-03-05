use std::{io::Write, path::Path};
use tracing::{level_filters::LevelFilter, subscriber::DefaultGuard};
use tracing_appender::{
    non_blocking::{NonBlocking, WorkerGuard},
    rolling::RollingFileAppender,
};
use tracing_subscriber::{fmt::format::DefaultFields, FmtSubscriber};

use crate::super_agent::{config::AgentID, defaults::SUB_AGENT_LOG_DIR};

use super::format::SubAgentFileLogger;

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

pub struct FileAppender<W = RollingFileAppender>(W)
where
    W: Write + Send + 'static;

impl FileAppender<RollingFileAppender> {
    pub fn new(agent_id: &AgentID, file_prefix: impl AsRef<Path>) -> Self {
        let logging_path = Path::new(SUB_AGENT_LOG_DIR).join(agent_id);
        let file_appender = tracing_appender::rolling::hourly(logging_path, file_prefix);
        Self(file_appender)
    }

    pub fn new_with_fixed_file(
        agent_id: &AgentID,
        dir_path: impl AsRef<Path>,
        file_prefix: impl AsRef<Path>,
    ) -> Self {
        let logging_path = dir_path.as_ref().join(agent_id);
        let file_appender = tracing_appender::rolling::never(logging_path, file_prefix);
        Self(file_appender)
    }
}

impl<W> Write for FileAppender<W>
where
    W: Write + Send + 'static,
{
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl<W> From<W> for FileAppender<W>
where
    W: Write + Send + 'static,
{
    fn from(writer: W) -> Self {
        Self(writer)
    }
}

pub struct FileLogger {
    file_subscriber: FmtSubscriber<DefaultFields, SubAgentFileLogger, LevelFilter, NonBlocking>,
    _guard: WorkerGuard,
}

pub struct LoggerGuard {
    _default_guard: DefaultGuard,
    _worker_guard: WorkerGuard,
}

impl FileLogger {
    /// Enables file logging for the current thread. This disables the global logger defined previously.
    /// To restore the previous global logger, the returned guard must be dropped.
    pub fn set_file_logging(self) -> LoggerGuard {
        let default_guard = tracing::subscriber::set_default(self.file_subscriber);
        LoggerGuard {
            _default_guard: default_guard,
            _worker_guard: self._guard,
        }
    }

    /// Logs with the file logger only the events generated within the `log_scope` closure.
    pub fn log_scope<T>(self, log_scope: impl FnOnce() -> T) -> T {
        // Any trace events (e.g. `info!`) generated in the `log_scope` closure or by functions it calls will be collected by the subscriber stored in the FileLogger.
        tracing::subscriber::with_default(self.file_subscriber, log_scope)
        // Exiting `log_scope` will drop the FileLogger's `_guard` and any remaining logs for `file_subscriber` should get flushed
    }
}

impl<W> From<W> for FileLogger
where
    W: Write + Send + 'static,
{
    fn from(appender: W) -> Self {
        let (non_blocking, _guard) = tracing_appender::non_blocking(appender);
        let file_subscriber = tracing_subscriber::fmt()
            .event_format(SubAgentFileLogger)
            .with_writer(non_blocking)
            .finish();
        Self {
            file_subscriber,
            _guard,
        }
    }
}
