//! [`tracing_subscriber`] layers used to report instrumentation to the different destinations
//! (stderr, files and OpenTelemetry).

pub mod file;
pub mod otel;
pub mod stderr;
