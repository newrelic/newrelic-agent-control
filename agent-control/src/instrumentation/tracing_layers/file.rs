use crate::instrumentation::config::logs::config::{LoggingConfig, LoggingConfigError};
use crate::instrumentation::tracing::{LayerBox, TracingGuard};
use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::Layer;
use tracing_subscriber::fmt::format::PrettyFields;
use tracing_subscriber::fmt::time::ChronoLocal;

pub type FileTracingExporter = WorkerGuard;

// Allow using the file guard as tracing exporter in order to keep it alive while the application
// reports instrumentation.
impl TracingGuard for FileTracingExporter {}

/// Returns an Optional [LayerBox] corresponding to a file output and the corresponding [WorkerGuard].
/// The result will be None if the file logger is not enabled.
pub fn file(
    config: &LoggingConfig,
    default_dir: PathBuf,
) -> Result<Option<(LayerBox, FileTracingExporter)>, LoggingConfigError> {
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
                .with_span_events(config.fmt_span_events())
                .with_target(target)
                .with_timer(ChronoLocal::new(timestamp_fmt.clone()))
                .fmt_fields(PrettyFields::new())
                .with_filter(config.filter()?)
                .boxed();
            Ok((layer, guard))
        })
        .transpose()
}
