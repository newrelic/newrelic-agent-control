//! OS process management for on-host executables: spawning, logging, restart policy, and shutdown.

pub mod command_os;
pub mod error;
pub mod executable_data;
pub mod logging;
#[cfg(target_os = "linux")]
pub mod process_discovery;
pub mod process_record;
pub mod restart_policy;
