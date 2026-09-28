//! Cross-platform OS distro-id detection. Linux only today; always `None`
//! elsewhere.
//!
//! `os.version` alone can collide across distros (e.g. Debian 12 and
//! openSUSE Leap 12 both report `VERSION_ID=12`), so this pairs with it to
//! disambiguate. Windows doesn't need an equivalent: `os.type=windows` plus
//! the `CurrentBuildNumber` from `os_version` already identifies a specific
//! Windows release unambiguously, there's no second Windows-like OS it could
//! collide with.

/// The distro's `ID` from `/etc/os-release` (e.g. "debian", "ubuntu").
#[cfg(target_os = "linux")]
pub fn detect_os_name() -> Option<String> {
    super::os_release::detect_os_id()
}

/// Always `None`: no distro-id detection support on this target.
#[cfg(not(target_os = "linux"))]
pub fn detect_os_name() -> Option<String> {
    None
}
