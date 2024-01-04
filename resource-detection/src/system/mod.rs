//! System resource detector
pub mod detector;

mod hostname;
mod identifier_machine_id_unix;

/// HOSTNAME_KEY represents the hostname key attribute
pub const HOSTNAME_KEY: &str = "hostname";
/// MACHINE_ID_KEY represents the machine_id key attribute
pub const MACHINE_ID_KEY: &str = "machine_id";
