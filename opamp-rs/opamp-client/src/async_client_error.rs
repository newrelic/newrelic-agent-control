//! This module defines a set of error types and result types related to the OpAMP client and management.
//!
//! It includes error types for various scenarios, such as errors in sending data over channels, process message errors,
//! synchronization state errors, and more. Additionally, it defines result types for operations involving OpAMP clients
//! and management.
//!
//! The module also provides a set of custom error types derived from the `thiserror::Error` trait, allowing for easy
//! error handling and propagation throughout the OpAMP-related code.
use thiserror::Error;
use tokio::{sync::mpsc::error::SendError, task::JoinError};

use crate::common::clientstate::SyncedStateError;
use crate::common::message_processor::ProcessError;

use crate::http::HttpClientError;
use tracing::error;

/// A type alias for results from OpAMP sender operations.
pub type OpampSenderResult<T> = Result<T, HttpClientError>;

/// A type alias for results from OpAMP operations.
pub type ClientResult<T> = Result<T, ClientError>;

/// A type alias for results from the OpAMP not started client.
pub type NotStartedClientResult<T> = Result<T, NotStartedClientError>;

/// A type alias for results from the OpAMP started client.
pub type StartedClientResult<T> = Result<T, StartedClientError>;
