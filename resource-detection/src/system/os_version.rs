//! OS version detection: the distro's `VERSION_ID` on Linux, the build number
//! on Windows, and `None` on any other target.

#[cfg(target_os = "linux")]
pub(super) mod linux;
#[cfg(target_os = "windows")]
mod windows;

/// The distro's `VERSION_ID` from `/etc/os-release` (e.g. "11").
#[cfg(target_os = "linux")]
pub fn detect_os_version() -> Option<String> {
    linux::detect_os_version()
}

/// The Windows build number from the registry (e.g. "26100").
#[cfg(target_os = "windows")]
pub fn detect_os_version() -> Option<String> {
    windows::detect_os_version()
}

/// Always `None`: no version detection on this target.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn detect_os_version() -> Option<String> {
    None
}
