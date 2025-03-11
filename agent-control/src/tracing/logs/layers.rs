use crate::tracing::logs::config::{LoggingConfig, LoggingConfigError};
use std::path::PathBuf;
use thiserror::Error;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::format::PrettyFields;
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::{Layer, Registry};

/// An enum representing possible errors during the logging initialization.
#[derive(Error, Debug)]
pub enum LoggingLayersInitError {
    #[error("error configuring logging `{0}`")]
    LoggingConfigError(#[from] LoggingConfigError),
}

pub type FileLoggerGuard = Option<WorkerGuard>;
pub type RegistryBox = Box<dyn Layer<Registry> + Send + Sync + 'static>;

pub struct LoggingLayersInitializer;

impl LoggingLayersInitializer {
    pub fn try_init(
        config: LoggingConfig,
        default_dir: PathBuf,
    ) -> Result<(RegistryBox, FileLoggerGuard), LoggingLayersInitError> {
        let target = config.format.target;
        let timestamp_fmt = config.format.timestamp.0.clone();

        // Construct the file logging layer and its worker guard, only if file logging is enabled.
        // Note we can actually specify different settings for each layer (log level, format, etc),
        // hence we repeat the logic here.
        let logging_filter = config.logging_filter()?;
        let (maybe_file_layer, file_logger_guard) = config.file.clone().setup(default_dir)?.map_or(
            Default::default(),
            |(file_writer, guard)| {
                let file_layer = tracing_subscriber::fmt::layer()
                    .with_writer(file_writer)
                    .with_ansi(false) // Disable colors for file
                    .with_target(target)
                    .with_timer(ChronoLocal::new(timestamp_fmt.clone()))
                    .fmt_fields(PrettyFields::new())
                    .with_filter(logging_filter);
                (Some(file_layer), Some(guard))
            },
        );

        let console_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stdout)
            .with_target(target)
            .with_timer(ChronoLocal::new(timestamp_fmt))
            .fmt_fields(PrettyFields::new())
            .with_filter(config.logging_filter()?);

        // Using a box for the layers doesn't allow having one of the layers as None or none of the
        // layers will be computed, so we only add the file_layer to the box if it's some.
        let mut layers = console_layer.boxed();
        if let Some(file_layer) = maybe_file_layer {
            layers = layers.and_then(file_layer).boxed();
        }

        Ok((layers, file_logger_guard))
    }
}
