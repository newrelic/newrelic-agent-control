use std::collections::HashMap;

use crate::context::Context;
use crate::sub_agent::restart_policy::RestartPolicy;
use crate::super_agent::config::AgentID;

#[derive(Debug, Clone)]
pub struct SupervisorConfigOnHost {
    pub(super) id: AgentID,
    pub(super) ctx: Context<bool>,
    pub(crate) bin: String,
    pub(super) args: Vec<String>,
    pub(super) env: HashMap<String, String>,
    pub(super) restart_policy: RestartPolicy,
    pub(super) log_to_file: bool,
}

impl SupervisorConfigOnHost {
    pub fn new(
        id: AgentID,
        exec: ExecutableData,
        ctx: Context<bool>,
        restart_policy: RestartPolicy,
        log_to_file: bool,
    ) -> Self {
        let ExecutableData { bin, args, env } = exec;
        SupervisorConfigOnHost {
            id,
            ctx,
            bin,
            args,
            env,
            restart_policy,
            log_to_file,
        }
    }
}

pub struct ExecutableData {
    bin: String,
    args: Vec<String>,
    env: HashMap<String, String>,
}

impl ExecutableData {
    pub fn new(bin: String) -> Self {
        ExecutableData {
            bin,
            args: Vec::default(),
            env: HashMap::default(),
        }
    }

    pub fn with_args(self, args: Vec<String>) -> Self {
        Self { args, ..self }
    }

    pub fn with_env(self, env: HashMap<String, String>) -> Self {
        Self { env, ..self }
    }
}
