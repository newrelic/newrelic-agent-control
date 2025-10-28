use crate::system::detector::SystemDetectorError;
use winreg::{
    RegKey,
    enums::{HKEY_LOCAL_MACHINE, KEY_READ},
};

const CRYPTOGRAPHY_KEY_PATH: &str = "SOFTWARE\\Microsoft\\Cryptography";
const MACHINE_GUID_KEY_NAME: &str = "MachineGuid";

#[derive(Default)]
pub struct MachineIdentityProvider {}

impl MachineIdentityProvider {
    /// Reads the _MachineGuid_ from the Windows registry using [winreg].
    pub fn provide(&self) -> Result<String, SystemDetectorError> {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE); // Open the local machine root key.
        // Open the Cryptography reg key with read permissions
        let cryptography_key = hklm
            .open_subkey_with_flags(CRYPTOGRAPHY_KEY_PATH, KEY_READ)
            .map_err(|err| SystemDetectorError::MachineIDError(err.to_string()))?;
        // Get the value from the registry
        cryptography_key
            .get_value(MACHINE_GUID_KEY_NAME)
            .map_err(|err| SystemDetectorError::MachineIDError(err.to_string()))
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
