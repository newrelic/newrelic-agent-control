use crate::agent_type::health_config::HealthConfig;
use crate::context::Context;
use crate::sub_agent::on_host::supervisor::restart_policy::RestartPolicy;
use crate::super_agent::config::AgentID;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SupervisorConfigOnHost {
    pub(super) id: AgentID,
    pub(super) ctx: Context<bool>,
    pub(crate) bin: String,
    pub(super) args: Vec<String>,
    pub(super) env: HashMap<String, String>,
    pub(super) restart_policy: RestartPolicy,
    pub(super) log_to_file: bool,
    pub(super) health: Option<HealthConfig>,
}

impl SupervisorConfigOnHost {
    pub fn new(
        id: AgentID,
        exec: ExecutableData,
        ctx: Context<bool>,
        restart_policy: RestartPolicy,
    ) -> Self {
        let ExecutableData { bin, args, env } = exec;
        SupervisorConfigOnHost {
            id,
            ctx,
            bin,
            args,
            env,
            restart_policy,
            log_to_file: false,
            health: None,
        }
    }

    pub fn with_file_logging(self, log_to_file: bool) -> Self {
        Self {
            log_to_file,
            ..self
        }
    }

    pub fn with_health_check(self, health: HealthConfig) -> Self {
        Self {
            health: Some(health),
            ..self
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
