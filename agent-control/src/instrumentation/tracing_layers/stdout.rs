use crate::instrumentation::config::logs::config::{LoggingConfig, LoggingConfigError};
use crate::instrumentation::tracing::LayerBox;
use tracing_subscriber::fmt::format::PrettyFields;
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::Layer;

/// Returns the [LayerBox] corresponding to the standard output.
pub fn stdout(config: &LoggingConfig) -> Result<LayerBox, LoggingConfigError> {
    let target = config.format.target;
    let timestamp_fmt = config.format.timestamp.0.clone();

    let layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout)
        .with_ansi(config.format.ansi_colors)
        .with_target(target)
        .with_timer(ChronoLocal::new(timestamp_fmt))
        .fmt_fields(PrettyFields::new())
        .with_filter(config.filter()?)
        .boxed();
    Ok(layer)
}
