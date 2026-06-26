//! [ExecutableData]: the binary, arguments, environment, restart policy, and shutdown timeout for a supervised process.

use crate::sub_agent::on_host::command::restart_policy::RestartPolicy;
use std::{collections::HashMap, time::Duration};

const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

/// Describes a supervised executable and how it should be launched and stopped.
#[derive(Clone)]
pub struct ExecutableData {
    /// Identifier for this executable within the agent.
    pub id: String,
    /// Path to the binary to run.
    pub bin: String,
    /// Command-line arguments passed to the binary.
    pub args: Vec<String>,
    /// Environment variables set for the process.
    pub env: HashMap<String, String>,
    /// Restart policy applied when the process exits.
    pub restart_policy: RestartPolicy,
    /// Time to wait for a graceful shutdown before forcing termination.
    pub shutdown_timeout: Duration,
}

impl ExecutableData {
    /// Creates executable data for the given id and binary with default args, env, and policy.
    pub fn new(id: String, bin: String) -> Self {
        ExecutableData {
            id,
            bin,
            args: Vec::default(),
            env: HashMap::default(),
            restart_policy: RestartPolicy::default(),
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
        }
    }

    /// Returns a copy with the given command-line arguments.
    pub fn with_args(self, args: Vec<String>) -> Self {
        Self { args, ..self }
    }

    /// Returns a copy with the given environment variables.
    pub fn with_env(self, env: HashMap<String, String>) -> Self {
        Self { env, ..self }
    }

    /// Returns a copy with the given restart policy.
    pub fn with_restart_policy(self, restart_policy: RestartPolicy) -> Self {
        Self {
            restart_policy,
            ..self
        }
    }
}
