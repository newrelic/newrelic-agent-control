//! Reads the Windows build number from the registry.
//!
//! `CurrentBuildNumber` is the standard, stable identifier for exactly which
//! Windows/Windows Server release is installed (e.g. "26100"). Unlike
//! `GetVersionEx`, reading the registry directly isn't affected by application
//! compatibility manifests lying about the OS version.

use windows_registry::LOCAL_MACHINE;

const CURRENT_VERSION_KEY_PATH: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion";
const CURRENT_BUILD_NUMBER_KEY_NAME: &str = "CurrentBuildNumber";

/// Reads the current build number from the Windows registry (e.g. "26100").
/// `None` if the key is missing or unreadable.
pub fn detect_os_version() -> Option<String> {
    LOCAL_MACHINE
        .open(CURRENT_VERSION_KEY_PATH)
        .and_then(|key| key.get_string(CURRENT_BUILD_NUMBER_KEY_NAME))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_build_number() {
        let version = detect_os_version()
            .unwrap_or_else(|| panic!("Unexpected error obtaining Windows build number"));
        assert!(!version.is_empty());
    }
}
