use std::sync::mpsc::Sender;

use crate::{command::stream::Event, config::agent_configs::AgentConfig, context::Context};

use super::{restart::BackoffStrategy, runner::SupervisorRunner};

const NRDOT_PATH: &str = "/usr/bin/nr-otel-collector";
const NRDOT_CONFIG_PATH: &str = "/etc/nr-otel-collector/config.yaml";
const NRDOT_ARGS: [&str; 3] = [
    "--config",
    NRDOT_CONFIG_PATH,
    "--feature-gates=-pkg.translator.prometheus.NormalizeName",
];

pub struct NRDOTConfig {
    ctx: Context<bool>,
    snd: Sender<Event>,
    cfg: AgentConfig,
}

impl From<&NRDOTConfig> for SupervisorRunner {
    fn from(value: &NRDOTConfig) -> Self {
        SupervisorRunner::new(
            NRDOT_PATH.to_owned(),
            NRDOT_ARGS.iter().map(|&s| s.to_owned()).collect(),
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

impl NRDOTConfig {
    pub fn new(snd: Sender<Event>, cfg: AgentConfig) -> Self {
        NRDOTConfig {
            ctx: Context::new(),
            snd,
            cfg,
        }
    }
}
