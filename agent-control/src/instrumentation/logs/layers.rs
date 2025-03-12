use crate::instrumentation::logs::config::{LoggingConfig, LoggingConfigError};
use crate::instrumentation::tracing::LayerBox;
use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::format::PrettyFields;
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::Layer;

pub type FileGuard = WorkerGuard;

/// Returns the [RegistryLayer] corresponding to the standard output.
pub fn stdout(config: &LoggingConfig) -> Result<LayerBox, LoggingConfigError> {
    let target = config.format.target;
    let timestamp_fmt = config.format.timestamp.0.clone();

    let layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout)
        .with_target(target)
        .with_timer(ChronoLocal::new(timestamp_fmt))
        .fmt_fields(PrettyFields::new())
        .with_filter(config.logging_filter()?)
        .boxed();
    Ok(layer)
}

/// Returns an Option containg [RegistryLayer] corresponding to a file output and the corresponding [WorkerGuard].
/// The result will be None if the file logger is not enabled.
pub fn file(
    config: &LoggingConfig,
    default_dir: PathBuf,
) -> Result<Option<(LayerBox, WorkerGuard)>, LoggingConfigError> {
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

//impl LoggingLayersInitializer {
//    pub fn try_init(
//        config: LoggingConfig,
//        default_dir: PathBuf,
//    ) -> Result<(Vec<RegistryLayer>, FileLoggerGuard), LoggingLayersInitError> {
//        let target = config.format.target;
//        let timestamp_fmt = config.format.timestamp.0.clone();
//
//        // Construct the file logging layer and its worker guard, only if file logging is enabled.
//        // Note we can actually specify different settings for each layer (log level, format, etc),
//        // hence we repeat the logic here.
//        let logging_filter = config.logging_filter()?;
//        let (maybe_file_layer, file_logger_guard) = config.file.clone().setup(default_dir)?.map_or(
//            Default::default(),
//            |(file_writer, guard)| {
//                let file_layer = tracing_subscriber::fmt::layer()
//                    .with_writer(file_writer)
//                    .with_ansi(false) // Disable colors for file
//                    .with_target(target)
//                    .with_timer(ChronoLocal::new(timestamp_fmt.clone()))
//                    .fmt_fields(PrettyFields::new())
//                    .with_filter(logging_filter);
//                (Some(file_layer), Some(guard))
//            },
//        );
//
//        let console_layer = tracing_subscriber::fmt::layer()
//            .with_writer(std::io::stdout)
//            .with_target(target)
//            .with_timer(ChronoLocal::new(timestamp_fmt))
//            .fmt_fields(PrettyFields::new())
//            .with_filter(config.logging_filter()?);
//
//        let mut layers = Vec::from([console_layer.boxed()]);
//        if let Some(file_layer) = maybe_file_layer {
//            layers.push(file_layer.boxed());
//        }
//
//        Ok((layers, file_logger_guard))
//    }
//}

// TODO: Add tests?
