use std::collections::HashMap;

use system::detector::SystemDetectorError;

mod file_reader;
pub mod system;

pub mod common;
pub use common::{Key, Value};

/// The `Resource` struct encapsulates a detected resource as per some system detection logic.
///
/// Generics:
/// - `E`: Represents the Environment type
/// - `N`: The number of attributes associated with the Resource
///
/// # Fields:
/// - `attributes`: An array of tuples containing the attribute key-value pair and a Result
///   containing either the value string or a `DetectError` object if an error occurred.
/// - `environment`: A placeholder type (`PhantomData`) permitting `Resource` to use the
///   generic `E` without it needing to hold values of that type.
pub struct Resource {
    pub attributes: HashMap<Key, Value>,
}

impl Resource {
    pub fn get(&self, key: Key) -> Option<Value> {
        self.attributes.get(&key).cloned()
    }
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum DetectError {
    #[error("error detecting system resources `{0}`")]
    SystemError(#[from] SystemDetectorError),
}

/// The `Detect` trait defines the detection interface to be implemented
/// by types pertaining to system resource detection.
///
/// Generics:
/// - `E`: Represents the Environment type
/// - `N`: The number of attributes associated with the Resource
///
/// # Methods:
/// - `detect`: Returns a `Resource` structure detected by the implementer of this trait.
pub trait Detect {
    fn detect(&self) -> Result<Resource, DetectError>;
}
