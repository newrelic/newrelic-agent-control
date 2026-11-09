//! OS process management for on-host executables: spawning, logging, restart policy, and shutdown.

pub mod command_os;
pub mod error;
pub mod executable_data;
pub mod logging;
#[cfg(target_os = "windows")]
pub mod process_record;
pub mod restart_policy;
