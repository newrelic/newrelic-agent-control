//! Logging for on-host executable output: stdout/stderr forwarding and optional file logging.

pub mod file_logger;
pub(crate) mod logger;
pub(crate) mod thread;
