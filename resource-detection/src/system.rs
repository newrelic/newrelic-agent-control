//! System resource detector
pub mod detector;
/// hostname retriever
pub mod hostname;
mod machine_identifier;
/// os-release parser (Linux)
mod os_release;
/// cross-platform OS version detection
pub mod os_version;
/// Windows registry-based OS version detection
#[cfg(target_os = "windows")]
mod os_version_windows;

/// HOSTNAME_KEY represents the hostname key attribute
pub const HOSTNAME_KEY: &str = "hostname";
/// MACHINE_ID_KEY represents the machine_id key attribute
pub const MACHINE_ID_KEY: &str = "machine_id";
