use thiserror::Error;

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
}

pub trait StatusServer {
    fn run(self) -> Result<(), StatusServerError>;
}
