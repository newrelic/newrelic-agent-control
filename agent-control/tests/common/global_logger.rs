use std::path::PathBuf;
use std::sync::Once;

use newrelic_agent_control::agent_control::defaults::AGENT_CONTROL_LOG_DIR;
use newrelic_agent_control::tracing::logs::config::LoggingConfig;
use newrelic_agent_control::tracing::logs::layers::LoggingLayersInitializer;
use newrelic_agent_control::tracing::tracer::Tracer;

static INIT_LOGGER: Once = Once::new();

pub fn init_logger() {
    INIT_LOGGER.call_once(|| {
        let logging_config: LoggingConfig = serde_yaml::from_str(
            r#"
insecure_fine_grained_level: "newrelic_agent_control=debug,opamp_client=info,off"
        "#,
        )
        .unwrap();

        // init logging singleton
        let (logging_layers, _file_logger_guard) = LoggingLayersInitializer::try_init(
            logging_config,
            PathBuf::from(AGENT_CONTROL_LOG_DIR),
        )
        .unwrap();

        Tracer::try_init(logging_layers).unwrap();
    });
}
