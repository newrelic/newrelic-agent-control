use std::sync::mpsc::Sender;

use crate::{command::stream::Event, config::agent_configs::AgentConfig};

use super::{context::SupervisorContext, restart::BackoffStrategy, runner::SupervisorRunner};

const NEWRELIC_INFRA_PATH: &str = "/usr/bin/newrelic-infra";
const NEWRELIC_INFRA_CONFIG_PATH: &str = "/etc/newrelic-infra.yml";
const NEWRELIC_INFRA_ARGS: [&str; 2] = ["--config", NEWRELIC_INFRA_CONFIG_PATH];

pub struct NRIConfig {
    ctx: SupervisorContext,
    snd: Sender<Event>,
    cfg: Option<AgentConfig>,
}

impl From<&NRIConfig> for SupervisorRunner {
    fn from(value: &NRIConfig) -> Self {
        let mut supervisor = SupervisorRunner::new(
            NEWRELIC_INFRA_PATH.to_owned(),
            NEWRELIC_INFRA_ARGS.iter().map(|&s| s.to_owned()).collect(),
            value.ctx.clone(),
            value.snd.clone(),
        );
        supervisor = match value.cfg {
            Some(ref c) => supervisor.with_restart_policy(
                c.restart_policy.restart_exit_codes.clone(),
                BackoffStrategy::from(&c.restart_policy.backoff_strategy),
            ),
            None => supervisor,
        };

        supervisor
    }
}

impl NRIConfig {
    pub fn new(ctx: SupervisorContext, snd: Sender<Event>, cfg: Option<AgentConfig>) -> Self {
        NRIConfig { ctx, snd, cfg }
    }
}
