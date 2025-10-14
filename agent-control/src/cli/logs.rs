use std::path::PathBuf;

use tracing::Level;

use crate::{
    agent_control::defaults::AGENT_CONTROL_LOG_DIR,
    cli::error::CliError,
    instrumentation::{
        config::logs::config::LoggingConfig,
        tracing::{TracingConfig, TracingGuardBox, try_init_tracing},
    },
};

/// Initializes logging (though the tracing crate) for the cli.
pub fn init(log_level: Level) -> Result<Vec<TracingGuardBox>, CliError> {
    let logging_config: LoggingConfig = serde_yaml::from_str(&format!("level: {}", log_level))
        .expect("Logging config should be valid");
    let tracing_config = TracingConfig::from_logging_path(PathBuf::from(AGENT_CONTROL_LOG_DIR))
        .with_logging_config(logging_config);
    try_init_tracing(tracing_config).map_err(CliError::from)
}
