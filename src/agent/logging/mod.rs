use super::error::AgentError;

pub struct Logging;

impl Logging {
    pub fn init() -> Result<(), AgentError> {
        tracing_subscriber::fmt().try_init().map_err(|_| {
            AgentError::LoggingError("unable to set agent global logging subscriber".to_string())
        })
    }
}
