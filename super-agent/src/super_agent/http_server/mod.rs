use std::sync::mpsc::RecvError;
use thiserror::Error;
use tokio::task::JoinError;

pub mod async_bridge;
pub mod config;
pub mod server;
pub(super) mod status;
pub(super) mod status_handler;
mod status_updater;

#[derive(Error, Debug)]
pub enum StatusServerError {
    #[error("status server error {0}")]
    StatusServerError(String),
    #[error("error building the server {0}")]
    BuildingServerError(String),
    #[error("error receiving server handle {0}")]
    ServerConsumerError(#[from] RecvError),
    #[error("error waiting for join handle {0}")]
    JoinHandleError(#[from] JoinError),
}

pub trait StatusServer {
    fn run(self) -> Result<(), StatusServerError>;
}
