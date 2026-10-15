//! OS version and distro-id detection.
//!
//! ## OS version
//!
//! This is the `VERSION_ID` on Linux, the build number
//! on Windows, and `None` on any other target.
//!
//! ## Distro ID
//!
//! Linux only today; always `None` elsewhere.
//!
//! `os.version` alone can collide across distros (e.g. Debian 12 and
//! openSUSE Leap 12 both report `VERSION_ID=12`), so this pairs with it to
//! disambiguate. Windows doesn't need it, since its build number is already
//! unambiguous.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

/// The distro's `VERSION_ID` from `/etc/os-release` on Linux, the build number from the
/// registry on Windows (e.g. "11", "26100"), or `None` on any other target.
#[cfg(target_os = "linux")]
pub use linux::detect_os_version;
#[cfg(target_os = "windows")]
pub use windows::detect_os_version;

/// Always `None`: no version detection on this target.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn detect_os_version() -> Option<String> {
    None
}

/// The distro's `ID` from `/etc/os-release` (e.g. "debian", "ubuntu").
#[cfg(target_os = "linux")]
pub use linux::detect_os_id as detect_os_name;

/// Always `None`: no distro-id detection support on this target.
#[cfg(not(target_os = "linux"))]
pub fn detect_os_name() -> Option<String> {
    None
}
