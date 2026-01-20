use crate::agent_type::runtime_config::restart_policy::rendered::RestartPolicyConfig;
use serde::Deserialize;
use std::collections::HashMap;

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

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Env(pub HashMap<String, String>);

#[derive(Debug, Default, Deserialize, Clone, PartialEq)]
pub struct Args(pub Vec<String>);
