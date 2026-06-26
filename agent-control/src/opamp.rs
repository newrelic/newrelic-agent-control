//! OpAMP client integration: building and running the OpAMP HTTP client, handling its
//! callbacks, reporting effective configuration, and validating signed remote configurations.
pub mod attributes;
pub mod auth;
pub mod callbacks;
pub mod client_builder;
pub mod effective_config;
pub mod http;
pub mod instance_id;
pub mod operations;
pub mod remote_config;

/// HTTP-like status code reported as the last error of an OpAMP operation.
pub type LastErrorCode = u16;
/// Human-readable message describing the last error of an OpAMP operation.
pub type LastErrorMessage = String;
