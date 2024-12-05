use std::path::PathBuf;
use std::sync::Once;

use newrelic_super_agent::logging::config::LoggingConfig;
use newrelic_super_agent::super_agent::defaults::SUPER_AGENT_LOG_DIR;

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
            .try_init(PathBuf::from(SUPER_AGENT_LOG_DIR))
            .unwrap();
    });
}
