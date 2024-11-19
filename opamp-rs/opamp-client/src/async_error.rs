//! This module defines a set of error types and result types related to the OpAMP client and management.
//!
//! It includes error types for various scenarios, such as errors in sending data over channels, process message errors,
//! synchronization state errors, and more. Additionally, it defines result types for operations involving OpAMP clients
//! and management.
//!
//! The module also provides a set of custom error types derived from the `thiserror::Error` trait, allowing for easy
//! error handling and propagation throughout the OpAMP-related code.
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;

use crate::common::clientstate::SyncedStateError;
use crate::common::message_processor::ProcessError;

use crate::http::TickerError;
use crate::http::{AsyncHttpClientError, AsyncTickerError};
use tracing::error;

/// Represents various errors that can occur during OpAMP connections.
#[derive(Error, Debug)]
pub enum AsyncClientError {
    /// Indicates a poison error, where a thread panicked while holding a lock.
    #[error("poison error, a thread panicked while holding a lock")]
    PoisonError,
    /// Indicates an error while fetching effective configuration.
    #[error("error while fetching effective config")]
    EffectiveConfigError,
    /// Represents a send error when communicating over channels.
    #[error("`{0}`")]
    SendError(#[from] SendError<()>),
    /// Represents an HTTP client error.
    #[error("`{0}`")]
    SenderError(#[from] AsyncHttpClientError),
    /// Represents a process message error.
    #[error("`{0}`")]
    ProcessMessageError(#[from] ProcessError),
    /// Represents a synchronized state error.
    #[error("`{0}`")]
    SyncedStateError(#[from] SyncedStateError),
    /// Indicates that the report effective configuration capability is not set.
    #[error("report effective configuration capability is not set")]
    UnsetEffectConfigCapability,
    /// Indicates that the report remote config status capability is not set.
    #[error("report remote configuration status capability is not set")]
    UnsetRemoteConfigStatusCapability,
    /// Indicates that the report health capability are not set.
    #[error("report health capability is not set")]
    UnsetHealthCapability,
    /// Error to use when the `on_connect_failed` callback has been called with this error type, which would consume its value.
    #[error("Client error. Handling via `on_connect_failed`.")]
    ConnectFailedCallback,
    /// Represents an internal ticker error.
    #[error("`{0}`")]
    TickerError(#[from] AsyncTickerError),
}

/// Represents errors related to the OpAMP started client.
#[derive(Error, Debug)]
pub enum AsyncStartedClientError {
    /// Represents a join error.
    #[error("error while joining internal thread")]
    JoinError,

    /// Represents errors in the HTTP client.
    #[error("`{0}`")]
    SenderError(#[from] AsyncHttpClientError),

    #[cfg(feature = "async")]
    /// Represents a client error in the OpAMP started client.
    #[error("`{0}`")]
    ClientError(#[from] AsyncClientError),
    /// Represents an internal ticker error.
    #[error("`{0}`")]
    AsyncTickerError(#[from] AsyncTickerError),
    /// Represents an internal ticker error.
    #[error("`{0}`")]
    TickerError(#[from] TickerError),
}

/// A type alias for results from OpAMP Async sender operations.
pub type OpampAsyncSenderResult<T> = Result<T, AsyncHttpClientError>;

/// A type alias for results from OpAMP operations.
pub type AsyncClientResult<T> = Result<T, AsyncClientError>;

/// A type alias for results from the OpAMP started client.
pub type AsyncStartedClientResult<T> = Result<T, AsyncStartedClientError>;
