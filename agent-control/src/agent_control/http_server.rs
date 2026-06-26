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
