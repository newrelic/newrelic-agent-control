//! OS process management for on-host executables: spawning, logging, restart policy, and shutdown.

pub mod command_os;
pub mod error;
pub mod executable_data;
pub mod logging;
pub mod restart_policy;
