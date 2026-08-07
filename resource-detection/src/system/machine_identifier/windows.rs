use crate::system::detector::SystemDetectorError;
use windows_registry::LOCAL_MACHINE;

const CRYPTOGRAPHY_KEY_PATH: &str = "SOFTWARE\\Microsoft\\Cryptography";
const MACHINE_GUID_KEY_NAME: &str = "MachineGuid";

#[derive(Default)]
pub struct MachineIdentityProvider {}

impl MachineIdentityProvider {
    /// Reads the _MachineGuid_ from the Windows registry.
    pub fn provide(&self) -> Result<String, SystemDetectorError> {
        LOCAL_MACHINE
            .open(CRYPTOGRAPHY_KEY_PATH)
            .and_then(|key| key.get_string(MACHINE_GUID_KEY_NAME))
            .map_err(|err| {
                SystemDetectorError::MachineIDError(format!(
                    "failed to read {MACHINE_GUID_KEY_NAME} from registry: {err}"
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_machine_guid() {
        let machine_guid = MachineIdentityProvider::default()
            .provide()
            .unwrap_or_else(|err| panic!("Unexpected error obtaining Windows MachineGuid: {err}"));
        assert!(!machine_guid.is_empty())
    }
}
