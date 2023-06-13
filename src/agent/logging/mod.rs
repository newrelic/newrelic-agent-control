use tracing_subscriber::fmt::try_init;

use super::error::AgentError;

pub struct Logging;

impl Logging {
    pub fn init() -> Result<(), AgentError> {
        try_init().map_err(|_| {
            AgentError::LoggingError("unable to set agent global logging subscriber".to_string())
        })
    }
}
