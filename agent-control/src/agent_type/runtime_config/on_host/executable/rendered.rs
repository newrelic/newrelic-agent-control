//! On-host executable definition after templating.
use crate::agent_type::runtime_config::restart_policy::rendered::RestartPolicyConfig;
use serde::Deserialize;
use std::collections::HashMap;

/// Rendered on-host executable.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Executable {
    /// Executable identifier for the health checker.
    pub id: String,
    /// Executable binary path. If not an absolute path, the PATH will be searched in an OS-defined way.
    pub path: String,
    /// Arguments passed to the executable.
    pub args: Args,
    /// Environmental variables passed to the process.
    pub env: Env,
    /// Defines how the executable will be restarted in case of failure.
    pub restart_policy: RestartPolicyConfig,
}

/// Rendered environment variables.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Env(
    /// The environment variable map.
    pub HashMap<String, String>,
);

/// Rendered command-line arguments.
#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct Args(
    /// The argument list.
    pub Vec<String>,
);
