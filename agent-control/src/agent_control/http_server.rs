//! Local status HTTP server exposing Agent Control and sub-agent health/status.

use std::sync::mpsc::RecvError;
use thiserror::Error;

pub mod async_bridge;
pub mod config;
pub mod runner;
pub mod server;
pub(super) mod status;
pub(super) mod status_handler;
mod status_updater;

/// Errors produced while building, starting or running the status HTTP server.
#[derive(Error, Debug)]
pub enum StatusServerError {
    /// The running status server returned an error.
    #[error("status server error {0}")]
    StatusServerError(String),
    /// Failed to build the server.
    #[error("error building the server {0}")]
    BuildingServerError(String),
    /// Failed to receive the server handle from its thread.
    #[error("error receiving server handle {0}")]
    ServerConsumerError(#[from] RecvError),
    /// Failed waiting for the async join handle.
    #[error("error waiting for async join handle {0}")]
    JoinHandleError(String),
    /// Failed to bind the server to its address.
    #[error("failed to bind HTTP server: {0}")]
    BindError(String),
    /// The server did not start within the allotted time.
    #[error("HTTP server startup timed out after {0:?}")]
    StartupTimeout(std::time::Duration),
    /// The server thread closed its startup channel before signalling readiness.
    #[error("HTTP server thread failed during startup")]
    StartupChannelClosed,
}

impl StatusServerError {
    /// Returns a stable, low-cardinality error code suitable for metric labels.
    pub fn error_kind(&self) -> &'static str {
        match self {
            StatusServerError::StatusServerError(_) => "status_server_error",
            StatusServerError::BuildingServerError(_) => "building_server_error",
            StatusServerError::ServerConsumerError(_) => "server_consumer_error",
            StatusServerError::JoinHandleError(_) => "join_handle_error",
            StatusServerError::BindError(_) => "bind_error",
            StatusServerError::StartupTimeout(_) => "startup_timeout",
            StatusServerError::StartupChannelClosed => "startup_channel_closed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::status_server_error(StatusServerError::StatusServerError("x".into()), "status_server_error")]
    #[case::building_server_error(
        StatusServerError::BuildingServerError("x".into()),
        "building_server_error"
    )]
    #[case::server_consumer_error(
        StatusServerError::ServerConsumerError(RecvError),
        "server_consumer_error"
    )]
    #[case::join_handle_error(StatusServerError::JoinHandleError("x".into()), "join_handle_error")]
    #[case::bind_error(StatusServerError::BindError("x".into()), "bind_error")]
    #[case::startup_timeout(
        StatusServerError::StartupTimeout(std::time::Duration::from_secs(1)),
        "startup_timeout"
    )]
    #[case::startup_channel_closed(
        StatusServerError::StartupChannelClosed,
        "startup_channel_closed"
    )]
    fn test_error_kind(#[case] err: StatusServerError, #[case] expected: &str) {
        assert_eq!(err.error_kind(), expected);
    }
}
