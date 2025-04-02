use std::path::PathBuf;
use std::sync::Once;

use newrelic_agent_control::agent_control::defaults::AGENT_CONTROL_LOG_DIR;
use newrelic_agent_control::instrumentation::config::logs::config::LoggingConfig;
use newrelic_agent_control::instrumentation::tracing::{try_init_tracing, TracingConfig};

static INIT_LOGGER: Once = Once::new();

pub fn init_logger() {
    INIT_LOGGER.call_once(|| {
        let logging_config: LoggingConfig = serde_yaml::from_str(
            r#"
insecure_fine_grained_level: "newrelic_agent_control=trace,opamp_client=info,off"
        "#,
        )
        .unwrap();

        let tracing_config = TracingConfig::from_logging_path(PathBuf::from(AGENT_CONTROL_LOG_DIR))
            .with_logging_config(logging_config);
        let _ = try_init_tracing(tracing_config).unwrap();
    });
}
