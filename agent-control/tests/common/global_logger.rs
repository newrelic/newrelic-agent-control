use std::path::PathBuf;
use std::sync::Once;

use newrelic_agent_control::agent_control::defaults::AGENT_CONTROL_LOG_DIR;
use newrelic_agent_control::logging::config::LoggingConfig;

static INIT_LOGGER: Once = Once::new();

pub fn init_logger() {
    INIT_LOGGER.call_once(|| {
        let logging_config: LoggingConfig = serde_yaml::from_str(
            r#"
level: debug        
        "#,
        )
        .unwrap();

        logging_config
            .try_init(PathBuf::from(AGENT_CONTROL_LOG_DIR))
            .unwrap();
    });
}
