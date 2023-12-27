use std::collections::HashMap;

use crate::{Detect, DetectError, Key, Resource, Value};

use super::{
    hostname::HostnameGetter, identifier_machine_id_unix::IdentifierProviderMachineId,
    HOSTNAME_KEY, MACHINE_ID_KEY,
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
impl Detect for SystemDetector {
    fn detect(&self) -> Result<Resource, DetectError> {
        Ok(Resource {
            attributes: HashMap::from([
                (
                    Key::from(HOSTNAME_KEY),
                    Value::from(
                        self.hostname_getter
                            .get()
                            .map(|val| val.into_string().unwrap_or_default())?,
                    ),
                ),
                (
                    Key::from(MACHINE_ID_KEY),
                    Value::from(self.machine_id_provider.provide()?),
                ),
            ]),
        })
    }
}
