//! System resource detector implementation
use super::{HOSTNAME_KEY, MACHINE_ID_KEY};
use crate::system::hostname::get_hostname;
use crate::system::machine_identifier::MachineIdentityProvider;
use crate::{DetectError, Detector, Key, Resource, Value};
use tracing::{error, instrument};

/// An enumeration of potential errors related to the system detector.
#[derive(thiserror::Error, Debug)]
pub enum SystemDetectorError {
    /// Error while getting hostname
    #[error("error getting hostname {0}")]
    HostnameError(String),
    /// Error while getting the machine-id
    #[error("error getting machine-id: {0}")]
    MachineIDError(String),
}

/// The `SystemDetector` struct encapsulates system detection functionality.
pub struct SystemDetector {
    machine_id_provider: MachineIdentityProvider,
}

/// Default implementation for `SystemDetector` struct.
impl Default for SystemDetector {
    fn default() -> Self {
        Self {
            machine_id_provider: MachineIdentityProvider::default(),
        }
    }
}

/// Returns the resources detected in the host system
impl Detector for SystemDetector {
    #[instrument(skip_all, name = "detect_system")]
    fn detect(&self) -> Result<Resource, DetectError> {
        Self::get_system_attributes(get_hostname, || self.machine_id_provider.provide())
    }
}

impl SystemDetector {
    /// Helper function to build the Resource according the results of getting the hostname and machin_id
    /// according to the provided getters.
    fn get_system_attributes<N, I>(
        hostname_getter: N,
        machine_id_getter: I,
    ) -> Result<Resource, DetectError>
    where
        N: Fn() -> Result<String, SystemDetectorError>,
        I: Fn() -> Result<String, SystemDetectorError>,
    {
        let mut collected_resources: Vec<(Key, Value)> = vec![];
        match hostname_getter() {
            Ok(hostname) => {
                collected_resources.push((Key::from(HOSTNAME_KEY), Value::from(hostname)))
            }
            Err(err) => error!(err_msg = %err, "getting hostname"),
        }

        match machine_id_getter() {
            Ok(machine_id) => {
                collected_resources.push((Key::from(MACHINE_ID_KEY), Value::from(machine_id)))
            }
            Err(err) => error!(err_msg = %err, "getting machine_id"),
        }
        Ok(Resource::new(collected_resources))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::both_success(
        || Ok("test-hostname".to_string()),
        || Ok("test-machine-id".to_string()),
        Some("test-hostname"),
        Some("test-machine-id")
    )]
    #[case::hostname_error(
        || Err(SystemDetectorError::HostnameError("failed".to_string())),
        || Ok("test-machine-id".to_string()),
        None,
        Some("test-machine-id")
    )]
    #[case::machine_id_error(
        || Ok("test-hostname".to_string()),
        || Err(SystemDetectorError::MachineIDError("failed".to_string())),
        Some("test-hostname"),
        None
    )]
    #[case::both_error(
        || Err(SystemDetectorError::HostnameError("failed".to_string())),
        || Err(SystemDetectorError::MachineIDError("failed".to_string())),
        None,
        None
    )]
    fn test_get_system_attributes(
        #[case] hostname_getter: fn() -> Result<String, SystemDetectorError>,
        #[case] machine_id_getter: fn() -> Result<String, SystemDetectorError>,
        #[case] expected_hostname: Option<&str>,
        #[case] expected_machine_id: Option<&str>,
    ) {
        let resource = SystemDetector::get_system_attributes(hostname_getter, machine_id_getter)
            .expect("Unexpected failure");

        assert_eq!(
            resource.get(Key::from(HOSTNAME_KEY)).map(String::from),
            expected_hostname.map(|h| h.to_string())
        );
        assert_eq!(
            resource.get(Key::from(MACHINE_ID_KEY)).map(String::from),
            expected_machine_id.map(|m| m.to_string())
        );
    }
}
