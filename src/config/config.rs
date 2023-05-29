use std::fmt::Debug;
use std::result;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Error getting config: {0}")]
    Error(String),
}

/// The result type used by this library, defaulting to [`Error`][crate::Error]
/// as the error type.
pub type Result<T> = result::Result<T, Error>;

/// Describes the way to get a serialized Config
///
/// Implementations of this trait need a generic parameter V that will store the serialized values
/// for the agents configs. For example Config<serde_json::Value>
pub trait Getter<C: Debug> {
    fn get(&self) -> Result<C>;
}