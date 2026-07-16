//! Instrumentation for Agent Control: logging and self-instrumentation via OpenTelemetry.
//!
//! Provides the configuration types and the [`tracing_subscriber`] layers used to report logs,
//! metrics and traces to stderr, files and OpenTelemetry endpoints.

pub mod config;
pub(crate) mod flush;
pub mod metrics;
pub mod resource_sampler;
pub mod tracing;
pub mod tracing_layers;
