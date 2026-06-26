//! On-host health checkers (exec, file, and HTTP based) and their aggregation.
/// Health derived from supervised executables' reported health.
pub mod exec;
/// Health read from a file written by the agent.
pub mod file;
/// The aggregate on-host health checker and its variants.
pub mod health_checker;
/// Health derived from an HTTP endpoint.
pub mod http;
