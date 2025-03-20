use crate::instrumentation::config::logs::config::{LoggingConfig, LoggingConfigError};
use crate::instrumentation::tracing::{LayerBox, TracingExporter};
use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::format::PrettyFields;
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::Layer;

pub type FileExporter = WorkerGuard;

// TODO: add comment
impl TracingExporter for FileExporter {}

/// Returns an Optional [LayerBox] corresponding to a file output and the corresponding [WorkerGuard].
/// The result will be None if the file logger is not enabled.
pub fn file(
    config: &LoggingConfig,
    default_dir: PathBuf,
) -> Result<Option<(LayerBox, FileExporter)>, LoggingConfigError> {
    let target = config.format.target;
    let timestamp_fmt = config.format.timestamp.0.clone();

    config
        .file
        .clone()
        .setup(default_dir)?
        .map(|(file_writer, guard)| {
            let layer = tracing_subscriber::fmt::layer()
                .with_writer(file_writer)
                .with_ansi(false) // Disable colors for file
                .with_target(target)
                .with_timer(ChronoLocal::new(timestamp_fmt.clone()))
                .fmt_fields(PrettyFields::new())
                .with_filter(config.logging_filter()?)
                .boxed();
            Ok((layer, guard))
        })
        .transpose()
}
