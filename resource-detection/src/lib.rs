//! Representations of entities.
//!
//! A [Resource] is an immutable representation of the entity producing
//! telemetry as attributes. For example, a process producing telemetry that is
//! running in a container on Kubernetes has a Pod name, it is in a namespace
//! and possibly is part of a Deployment which also has a name. All three of
//! these attributes can be included in the `Resource`.

#![warn(missing_docs)]

use std::collections::HashMap;

use cloud::aws::detector::AWSDetectorError;
use system::detector::SystemDetectorError;

pub mod cloud;
pub mod system;

pub mod common;

use crate::cloud::azure::detector::AzureDetectorError;
pub use common::{Key, Value};

/// The `Resource` struct encapsulates a detected resource as per some detection logic.
///
/// # Fields:
/// - `attributes`: A HashMap of Key/Values
#[derive(Debug)]
pub struct Resource {
    attributes: HashMap<Key, Value>,
}

impl Resource {
    /// Create a new `Resource` from key value pairs.
    ///
    /// Values are de-duplicated by key, and the first key-value pair with a non-empty string value
    /// will be retained
    pub fn new<T: IntoIterator<Item = (Key, Value)>>(kvs: T) -> Self {
        let mut attributes = HashMap::new();

        for kv in kvs.into_iter() {
            attributes.insert(kv.0, kv.1);
        }

        Resource { attributes }
    }

    /// Retrieve the value from resource associate with given key.
    pub fn get(&self, key: Key) -> Option<Value> {
        self.attributes.get(&key).cloned()
    }
}

/// DetectError defines the issue found while retrieving the resource attributes.
#[derive(thiserror::Error, Debug)]
pub enum DetectError {
    /// Error for the system implementation
    #[error("error detecting system resources `{0}`")]
    SystemError(#[from] SystemDetectorError),
    /// Error for the AWS cloud implementation
    #[error("error detecting aws resources `{0}`")]
    AWSError(#[from] AWSDetectorError),
    /// Error for the Azure cloud implementation
    #[error("error detecting azure resources `{0}`")]
    AzureError(#[from] AzureDetectorError),
}

/// The `Detect` trait defines the detection interface to be implemented
/// by types pertaining to resource detection.
pub trait Detect {
    /// Returns a `Resource` structure detected by the implementer of this trait or
    /// DetectError if an error was found.
    fn detect(&self) -> Result<Resource, DetectError>;
}
