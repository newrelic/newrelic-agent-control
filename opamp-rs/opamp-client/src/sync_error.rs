//! This module defines a set of error types and result types related to the OpAMP client and management.

use crate::common::clientstate::SyncedStateError;
use crate::common::message_processor::ProcessError;
use crate::http::HttpClientError;
use crate::http::TickerError;
use thiserror::Error;

/// Represents various errors that can occur during OpAMP connections.
#[derive(Error, Debug)]
pub enum ClientError {
    /// Indicates a poison error, where a thread panicked while holding a lock.
    #[error("poison error, a thread panicked while holding a lock")]
    PoisonError,
    /// Error to use when the `on_connect_failed` callback has been called with this error type, which would consume its value.
    #[error("Client error. Handling via `on_connect_failed`.")]
    ConnectFailedCallback(String),
    /// Represents a process message error.
    #[error("`{0}`")]
    ProcessMessageError(#[from] ProcessError),
    /// Represents an HTTP client error.
    #[error("`{0}`")]
    SenderError(#[from] HttpClientError),
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
    /// Indicates an error while fetching effective configuration.
    #[error("error while fetching effective config")]
    EffectiveConfigError,
    /// Represents an internal ticker error.
    #[error("`{0}`")]
    TickerError(#[from] TickerError),
}

/// Represents errors related to the OpAMP started client.
#[derive(Error, Debug)]
pub enum StartedClientError {
    /// Represents a join error.
    #[error("error while joining internal thread")]
    JoinError,
    /// Represents a synchronized state error.
    #[error("`{0}`")]
    SyncClientError(#[from] ClientError),
    /// Represents an internal ticker error.
    #[error("`{0}`")]
    TickerError(#[from] TickerError),
}

/// A type alias for results from OpAMP operations.
pub type ClientResult<T> = Result<T, ClientError>;

/// A type alias for results from OpAMP Sync sender operations.
pub type OpampSenderResult<T> = Result<T, HttpClientError>;

/// A type alias for results from the OpAMP started client.
pub type StartedClientResult<T> = Result<T, StartedClientError>;
