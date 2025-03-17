use std::path::PathBuf;
use std::sync::Once;

use newrelic_agent_control::agent_control::defaults::AGENT_CONTROL_LOG_DIR;
use newrelic_agent_control::instrumentation::config::InstrumentationConfig;
use newrelic_agent_control::instrumentation::logs::config::LoggingConfig;
use newrelic_agent_control::instrumentation::tracing::{try_init_tracing, TracingConfig};

static INIT_LOGGER: Once = Once::new();

pub fn init_logger() {
    INIT_LOGGER.call_once(|| {
        let logging_config: LoggingConfig = serde_yaml::from_str(
            r#"
insecure_fine_grained_level: "newrelic_agent_control=debug,opamp_client=info,off"
        "#,
        )
        .unwrap();

        let tracing_config = TracingConfig::new(
            PathBuf::from(AGENT_CONTROL_LOG_DIR),
            logging_config,
            InstrumentationConfig::default(),
        );
        let _ = try_init_tracing(tracing_config).unwrap();
    });
}
