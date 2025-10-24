use crate::sub_agent::on_host::command::restart_policy::RestartPolicy;
use std::{collections::HashMap, time::Duration};

const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct ExecutableData {
    pub id: String,
    pub bin: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub restart_policy: RestartPolicy,
    pub shutdown_timeout: Duration,
}

impl ExecutableData {
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

    pub fn with_args(self, args: Vec<String>) -> Self {
        Self { args, ..self }
    }

    pub fn with_env(self, env: HashMap<String, String>) -> Self {
        Self { env, ..self }
    }

    pub fn with_restart_policy(self, restart_policy: RestartPolicy) -> Self {
        Self {
            restart_policy,
            ..self
        }
    }
}
