//! System resource detector
pub mod detector;
/// hostname retriever
pub mod hostname;
mod machine_identifier;
/// cross-platform OS distro-id detection
pub mod os_name;
/// OS version detection
pub mod os_version;

/// HOSTNAME_KEY represents the hostname key attribute
pub const HOSTNAME_KEY: &str = "hostname";
/// MACHINE_ID_KEY represents the machine_id key attribute
pub const MACHINE_ID_KEY: &str = "machine_id";
