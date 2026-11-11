//! OS process management for on-host executables: spawning, logging, restart policy, and shutdown.

pub mod error;
pub mod executable_data;
pub mod logging;
pub mod os_process;
pub mod restart_policy;
