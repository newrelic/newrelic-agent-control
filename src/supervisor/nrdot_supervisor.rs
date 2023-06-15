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
    ctx: Context,
    snd: Sender<Event>,
    cfg: Option<AgentConfig>,
}

impl From<&NRDOTConfig> for SupervisorRunner {
    fn from(value: &NRDOTConfig) -> Self {
        let mut supervisor = SupervisorRunner::new(
            NRDOT_PATH.to_owned(),
            NRDOT_ARGS.iter().map(|&s| s.to_owned()).collect(),
            value.ctx.clone(),
            value.snd.clone(),
        );
        // Additional configs if present
        if let Some(ref c) = value.cfg {
            supervisor = supervisor.with_restart_policy(
                c.restart_policy.restart_exit_codes.clone(),
                BackoffStrategy::from(&c.restart_policy.backoff_strategy),
            )
        }

        supervisor
    }
}

impl NRDOTConfig {
    pub fn new(ctx: Context, snd: Sender<Event>, cfg: Option<AgentConfig>) -> Self {
        NRDOTConfig { ctx, snd, cfg }
    }
}
