use serde::Deserialize;

/// Represents a custom time stamp format for logging.
#[derive(Debug, Deserialize, PartialEq, Clone)]
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

/// Represents the supported logging formatters
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "snake_case")]
pub enum Formatter {
    /// Human-readable single-line logs
    Default,
    /// Newline delimited JSON logs
    Json,
}

impl Default for Formatter {
    fn default() -> Self {
        Self::Default
    }
}

/// Defines the format to be used for logging, including target and timestamp.
///
/// # Fields:
/// - `target`: A bool that indicates whether or not the target of the trace event will be included in the formatted output.
/// - `timestamp`: Specifies a `TimestampFormat` the application will use for logging timestamps.
/// - `ansi_colors`: Specifies if ansi colors should be used in stdout logs.
#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
pub struct LoggingFormat {
    #[serde(default)]
    pub(crate) target: bool,
    #[serde(default)]
    pub(crate) timestamp: TimestampFormat,
    #[serde(default)]
    pub(crate) ansi_colors: bool,
    #[serde(default)]
    pub(crate) formatter: Formatter,
}

#[cfg(test)]
mod tests {
    use super::*;

    use rstest::rstest;
    use serde_yaml;

    #[rstest]
    #[case::with_defaults(
        r#"
        target: false
        "#,
        LoggingFormat {
            target: false,
            timestamp: TimestampFormat::default(),
            ansi_colors: false,
            formatter: Formatter::Default,
        }
    )]
    #[case::with_custom_values(
        r#"
        target: true
        timestamp: "custom_format"
        ansi_colors: true
        formatter: json
        "#,
        LoggingFormat {
            target: true,
            timestamp: TimestampFormat("custom_format".to_string()),
            ansi_colors: true,
            formatter: Formatter::Json,
        }
    )]
    #[case::with_partial_values(
        r#"
        target: true
        "#,
        LoggingFormat {
            target: true,
            timestamp: TimestampFormat::default(),
            ansi_colors: false,
            formatter: Formatter::Default,
        }
    )]
    fn test_logging_format_deserialization(
        #[case] yaml_data: &str,
        #[case] expected_logging_format: LoggingFormat,
    ) {
        let logging_format: LoggingFormat = serde_yaml::from_str(yaml_data).unwrap();

        assert_eq!(logging_format, expected_logging_format);
    }
}
