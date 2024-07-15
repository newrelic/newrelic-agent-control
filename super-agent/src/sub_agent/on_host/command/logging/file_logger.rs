use std::{
    io::Write,
    path::{Path, PathBuf},
};
use tracing::{level_filters::LevelFilter, subscriber::DefaultGuard};
use tracing_appender::{
    non_blocking::{NonBlocking, WorkerGuard},
    rolling::RollingFileAppender,
};
use tracing_subscriber::{
    fmt::format::{DefaultFields, Format, Full},
    FmtSubscriber,
};

use crate::super_agent::config::AgentID;

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
    pub fn new(agent_id: &AgentID, path: PathBuf, file_prefix: impl AsRef<Path>) -> Self {
        let file_appender = tracing_appender::rolling::hourly(path.join(agent_id), file_prefix);
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
    file_subscriber: FmtSubscriber<DefaultFields, Format<Full, ()>, LevelFilter, NonBlocking>,
    _guard: WorkerGuard,
}

pub struct SubAgentLoggerGuard {
    _default_guard: DefaultGuard,
    _worker_guard: WorkerGuard,
}

impl FileLogger {
    /// Enables file logging for the current thread. This disables the global logger defined previously.
    /// To restore the previous global logger, the returned guard must be dropped.
    pub fn set_file_logging(self) -> SubAgentLoggerGuard {
        let default_guard = tracing::subscriber::set_default(self.file_subscriber);
        SubAgentLoggerGuard {
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
}
