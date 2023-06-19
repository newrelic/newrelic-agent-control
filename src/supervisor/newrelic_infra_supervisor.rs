use std::sync::mpsc::Sender;

use crate::{command::stream::Event, config::agent_configs::AgentConfig, context::Context};

use super::{restart::BackoffStrategy, runner::SupervisorRunner};

const NEWRELIC_INFRA_PATH: &str = "/usr/bin/newrelic-infra";
const NEWRELIC_INFRA_CONFIG_PATH: &str = "/etc/newrelic-infra.yml";
const NEWRELIC_INFRA_ARGS: [&str; 2] = ["--config", NEWRELIC_INFRA_CONFIG_PATH];

pub struct NRIConfig {
    ctx: Context<bool>,
    snd: Sender<Event>,
    cfg: AgentConfig,
}

impl From<&NRIConfig> for SupervisorRunner {
    fn from(value: &NRIConfig) -> Self {
        SupervisorRunner::new(
            NEWRELIC_INFRA_PATH.to_owned(),
            NEWRELIC_INFRA_ARGS.iter().map(|&s| s.to_owned()).collect(),
            value.ctx.clone(),
            value.snd.clone(),
        )
        // Additional configs
        .with_restart_policy(
            value.cfg.restart_policy.restart_exit_codes.clone(),
            BackoffStrategy::from(&value.cfg.restart_policy.backoff_strategy),
        )
    }
}

impl NRIConfig {
    pub fn new(snd: Sender<Event>, cfg: AgentConfig) -> Self {
        NRIConfig {
            ctx: Context::new(),
            snd,
            cfg,
        }
    }
}
