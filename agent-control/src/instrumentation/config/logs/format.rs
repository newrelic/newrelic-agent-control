use serde::{Deserialize, Serialize};

/// Represents a custom time stamp format for logging.
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub(crate) struct TimestampFormat(pub(crate) String);

/// Provides a default `TimestampFormat`. The default format is based on
/// [chrono strftime](https://docs.rs/chrono/latest/chrono/format/strftime/index.html#fn7)
///
/// # Returns:
/// A new `TimestampFormat` instance with the format set as "%Y-%m-%dT%H:%M:%S".
impl Default for TimestampFormat {
    fn default() -> Self {
        Self("%Y-%m-%dT%H:%M:%S".to_string())
    }
}

/// Defines the format to be used for logging, including target and timestamp.
///
/// # Fields:
/// - `target`: A bool that indicates whether or not the target of the trace event will be included in the formatted output.
/// - `timestamp`: Specifies a `TimestampFormat` the application will use for logging timestamps.
/// - `ansi_colors`: Specifies if ansi colors should be used in stdout logs.
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, Default)]
pub struct LoggingFormat {
    #[serde(default)]
    pub(crate) target: bool,
    #[serde(default)]
    pub(crate) timestamp: TimestampFormat,
    #[serde(default)]
    pub(crate) ansi_colors: bool,
}
