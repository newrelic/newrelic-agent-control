//! OpAMP client common crate errors.

use thiserror::Error;

/// Represents errors that can occur on network operations
#[derive(Error, Debug)]
pub enum ConnectionError {
    /// Error when connecting via an Async HTTP client.
    #[cfg(feature = "async-http")]
    #[error(transparent)]
    AsyncHTTPClientError(#[from] crate::http::AsyncHttpClientError),
    #[cfg(feature = "sync-http")]
    /// Error when connecting via a Sync HTTP client.
    #[error(transparent)]
    HTTPClientError(#[from] crate::http::HttpClientError),
}

/// Represents errors that can occur in the OpAMP not started client.
#[derive(Error, Debug)]
pub enum NotStartedClientError {
    #[cfg(feature = "async-http")]
    /// Represents a client error in the OpAMP started client.
    #[error("`{0}`")]
    AsyncClientError(#[from] crate::AsyncClientError),

    #[cfg(feature = "sync-http")]
    /// Represents a client error in the OpAMP started client.
    #[error("`{0}`")]
    ClientError(#[from] crate::ClientError),
}

/// A type alias for results from the OpAMP not started client.
pub type NotStartedClientResult<T> = Result<T, NotStartedClientError>;
