//! System resource detector implementation
use fs::LocalFile;
use tracing::{error, instrument};

use crate::{DetectError, Detector, Key, Resource, Value};

use super::{
    HOSTNAME_KEY, MACHINE_ID_KEY, hostname::HostnameGetter,
    identifier_machine_id_unix::IdentifierProviderMachineId,
};

/// An enumeration of potential errors related to the system detector.
#[derive(thiserror::Error, Debug)]
pub enum SystemDetectorError {
    /// Error while getting hostname
    #[error("error getting hostname `{0}`")]
    HostnameError(String),
    /// Error while getting the machine-id
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
    machine_id_provider: IdentifierProviderMachineId<LocalFile>,
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
impl Detector for SystemDetector {
    #[instrument(skip_all, name = "detect_system")]
    fn detect(&self) -> Result<Resource, DetectError> {
        let mut collected_resources: Vec<(Key, Value)> = vec![];

        match self.hostname_getter.get() {
            Ok(hostname) => collected_resources.push((
                Key::from(HOSTNAME_KEY),
                Value::from(hostname.into_string().unwrap_or_default()),
            )),
            Err(err) => error!(err_msg = %err, "getting hostname"),
        }

        match self.machine_id_provider.provide() {
            Ok(machine_id) => {
                collected_resources.push((Key::from(MACHINE_ID_KEY), Value::from(machine_id)))
            }
            Err(err) => error!(err_msg = %err, "getting machine_id"),
        }

        Ok(Resource::new(collected_resources))
    }
}
