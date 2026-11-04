//! OS process management for on-host executables: spawning, log files, restart policy, and shutdown.

pub mod command_os;
pub mod error;
pub mod executable_data;
pub mod log_files;
pub mod restart_policy;
