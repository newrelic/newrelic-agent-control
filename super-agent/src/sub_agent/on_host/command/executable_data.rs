use crate::sub_agent::on_host::command::restart_policy::RestartPolicy;
use std::collections::HashMap;

#[derive(Clone)]
pub struct ExecutableData {
    pub bin: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub restart_policy: RestartPolicy,
}

impl ExecutableData {
    pub fn new(bin: String) -> Self {
        ExecutableData {
            bin,
            args: Vec::default(),
            env: HashMap::default(),
            restart_policy: RestartPolicy::default(),
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
