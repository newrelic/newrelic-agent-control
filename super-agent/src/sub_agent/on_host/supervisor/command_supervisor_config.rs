use crate::agent_type::health_config::OnHostHealthConfig;
use crate::context::Context;
use crate::sub_agent::on_host::supervisor::restart_policy::RestartPolicy;
use crate::super_agent::config::AgentID;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SupervisorConfigOnHost {
    pub(super) id: AgentID,
    pub(super) ctx: Context<bool>,
    pub(crate) exec_data: Option<ExecutableData>,
    pub(super) log_to_file: bool,
    pub(super) health: Option<OnHostHealthConfig>,
}

impl SupervisorConfigOnHost {
    pub fn new(id: AgentID, ctx: Context<bool>) -> Self {
        SupervisorConfigOnHost {
            id,
            ctx,
            exec_data: None,
            log_to_file: false,
            health: None,
        }
    }

    pub fn with_exec_data(self, exec_data: ExecutableData) -> Self {
        Self {
            exec_data: Some(exec_data),
            ..self
        }
    }

    pub fn with_file_logging(self, log_to_file: bool) -> Self {
        Self {
            log_to_file,
            ..self
        }
    }

    pub fn with_health_check(self, health: OnHostHealthConfig) -> Self {
        Self {
            health: Some(health),
            ..self
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecutableData {
    pub(crate) bin: String,
    pub(crate) args: Vec<String>,
    pub(crate) env: HashMap<String, String>,
    pub(crate) restart_policy: RestartPolicy,
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
