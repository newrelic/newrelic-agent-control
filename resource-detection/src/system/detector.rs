use std::marker::PhantomData;

use crate::{Detect, DetectError, Resource};

use super::{
    hostname::HostnameGetter, identifier_machine_id_unix::IdentifierProviderMachineId, System,
};

#[derive(thiserror::Error, Debug, Clone)]
pub enum SystemDetectorError {
    #[error("error getting hostname `{0}`")]
    HostnameError(String),
    #[error("error getting machine-id: `{0}`")]
    MachineIDError(String),
}

/// The `SystemDetector` struct encapsulates system detection functionality.
///
/// # Fields:
/// - `hostname_getter`: An instance of the `HostnameGetter` struct for retrieving system hostname.
/// - `machine_id_provider`: An instance of the `IdentifierProviderMachineId` struct for retrieving machine ID.
pub struct SystemDetector {
    hostname_getter: HostnameGetter,
    machine_id_provider: IdentifierProviderMachineId,
}

/// Default implementation for `SystemDetector` struct.
impl Default for SystemDetector {
    fn default() -> Self {
        Self {
            hostname_getter: HostnameGetter {},
            machine_id_provider: IdentifierProviderMachineId::default(),
        }
    }
}

/// Implementing the `Detect` trait for the `SystemDetector` struct.
impl Detect<System, 2> for SystemDetector {
    fn detect(&self) -> Resource<System, 2> {
        Resource {
            attributes: [
                (
                    "hostname".to_string(),
                    self.hostname_getter
                        .get()
                        .map(|val| val.into_string().unwrap_or_default())
                        .map_err(|e| SystemDetectorError::HostnameError(e.to_string()).into()),
                ),
                (
                    "machine-id".to_string(),
                    self.machine_id_provider.provide().map_err(|e| e.into()),
                ),
            ],
            environment: PhantomData,
        }
    }
}

/// Extension methods for the `Resource` struct when the Resource represents a system environment.
impl Resource<System, 2> {
    /// Attempts to get the hostname from the Resource attributes.
    ///
    /// # Returns:
    /// - `Ok(String)`: The hostname string if present and retrieval is successful.
    /// - `Err(DetectError)`: A `DetectError` instance in case of an error.
    pub fn get_hostname(&self) -> Result<String, DetectError> {
        self.attributes[0].1.clone()
    }

    /// Attempts to get the machine ID from the Resource attributes.
    ///
    /// # Returns:
    /// - `Ok(String)`: The machine ID string if present and retrieval is successful.
    /// - `Err(DetectError)`: A `DetectError` instance in case of an error.
    pub fn get_machine_id(&self) -> Result<String, DetectError> {
        self.attributes[1].1.clone()
    }
}
