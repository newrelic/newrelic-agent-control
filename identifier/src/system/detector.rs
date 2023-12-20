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

pub struct SystemDetector {
    hostname_getter: HostnameGetter,
    machine_id_provider: IdentifierProviderMachineId,
}

impl Default for SystemDetector {
    fn default() -> Self {
        Self {
            hostname_getter: HostnameGetter {},
            machine_id_provider: IdentifierProviderMachineId::default(),
        }
    }
}

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

impl Resource<System, 2> {
    pub fn get_hostname(&self) -> Result<String, DetectError> {
        self.attributes[0].1.clone()
    }

    pub fn get_machine_id(&self) -> Result<String, DetectError> {
        self.attributes[1].1.clone()
    }
}
