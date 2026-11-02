//! OS process management for on-host executables: spawning, log files, restart policy, and shutdown.

pub mod error;
pub mod executable_data;
pub mod log_files;
pub mod os_process;
pub mod restart_policy;
