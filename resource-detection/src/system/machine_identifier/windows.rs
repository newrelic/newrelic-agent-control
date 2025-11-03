use crate::system::detector::SystemDetectorError;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
};

const CRYPTOGRAPHY_KEY_PATH: &str = "SOFTWARE\\Microsoft\\Cryptography";
const MACHINE_GUID_KEY_NAME: &str = "MachineGuid";

#[derive(Default)]
pub struct MachineIdentityProvider {}

impl MachineIdentityProvider {
    /// Reads the _MachineGuid_ from the Windows registry.
    pub fn provide(&self) -> Result<String, SystemDetectorError> {
        // Convert the value names to wide strings (UTF-16)
        let key_path: Vec<u16> = CRYPTOGRAPHY_KEY_PATH
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let key_name: Vec<u16> = MACHINE_GUID_KEY_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        Self::read_string_from_registry(key_path, key_name)
    }

    /// Helper to read a string from the registry
    pub fn read_string_from_registry(
        key_path: Vec<u16>,
        key_name: Vec<u16>,
    ) -> Result<String, SystemDetectorError> {
        unsafe {
            // Open the registry key
            let mut registry_key: *mut std::ffi::c_void = std::ptr::null_mut();
            let result = RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                key_path.as_ptr(),
                0,
                KEY_READ,
                &mut registry_key,
            );

            if result != ERROR_SUCCESS {
                return Err(SystemDetectorError::MachineIDError(format!(
                    "failed to open registry key: error code {}",
                    result
                )));
            }

            // Query the type and size of the value
            let mut value_type = 0u32;
            let mut data_size = 0u32;

            let result = RegQueryValueExW(
                registry_key,
                key_name.as_ptr(),
                std::ptr::null(),
                &mut value_type,
                std::ptr::null_mut(),
                &mut data_size,
            );

            if result != ERROR_SUCCESS {
                Self::close_registry_key(registry_key)?;
                return Err(SystemDetectorError::MachineIDError(format!(
                    "failed to query registry value size: error code {}",
                    result
                )));
            }

            // Allocate buffer and read the actual value
            let mut buffer: Vec<u16> = vec![0; (data_size / 2) as usize];

            let result = RegQueryValueExW(
                registry_key,
                key_name.as_ptr(),
                std::ptr::null(),
                &mut value_type,
                buffer.as_mut_ptr() as *mut u8,
                &mut data_size,
            );

            Self::close_registry_key(registry_key)?;

            if result != ERROR_SUCCESS {
                return Err(SystemDetectorError::MachineIDError(format!(
                    "failed to read registry value: error code {}",
                    result
                )));
            }

            // Verify it's a string type
            if value_type != REG_SZ {
                return Err(SystemDetectorError::MachineIDError(format!(
                    "unexpected registry value type: {}",
                    value_type
                )));
            }

            // Convert from UTF-16 to String, removing the null terminator
            let string_value = String::from_utf16_lossy(&buffer)
                .trim_end_matches('\0')
                .to_string();

            Ok(string_value)
        }
    }

    /// Helper to close the registry key returning the corresponding error on failure
    unsafe fn close_registry_key(hkey: *mut std::ffi::c_void) -> Result<(), SystemDetectorError> {
        unsafe {
            let result = RegCloseKey(hkey);
            if result != ERROR_SUCCESS {
                return Err(SystemDetectorError::MachineIDError(format!(
                    "failed to close the registry key: error code {result}"
                )));
            }
            Ok(())
        }
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
