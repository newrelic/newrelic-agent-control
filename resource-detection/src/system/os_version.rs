//! Cross-platform OS version detection. Linux and Windows only today; always
//! `None` elsewhere (e.g. macOS).

/// The distro's `VERSION_ID` from `/etc/os-release` (e.g. "11").
#[cfg(target_os = "linux")]
pub fn detect_os_version() -> Option<String> {
    super::os_release::detect_os_version()
}

/// The Windows build number from the registry (e.g. "26100").
#[cfg(target_os = "windows")]
pub fn detect_os_version() -> Option<String> {
    super::os_version_windows::detect_os_version()
}

/// Always `None`: no version-detection support on this target.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn detect_os_version() -> Option<String> {
    None
}
