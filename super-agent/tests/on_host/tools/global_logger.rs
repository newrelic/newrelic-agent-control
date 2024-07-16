use std::sync::Once;

use newrelic_super_agent::logging::config::LoggingConfig;

static INIT_LOGGER: Once = Once::new();

pub fn init_logger() {
    INIT_LOGGER.call_once(|| {
        LoggingConfig::default().try_init().unwrap();
    });
}
