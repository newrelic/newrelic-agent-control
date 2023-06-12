use std::sync::mpsc::Sender;

use crate::{command::stream::Event, config::agent_configs::AgentConfig};

use super::{context::SupervisorContext, restart::BackoffStrategy, runner::SupervisorRunner};

const NEWRELIC_INFRA_PATH: &str = "/usr/bin/newrelic-infra"; // FIXME CHANGE TO NRDOT
const NEWRELIC_INFRA_CONFIG_PATH: &str = "/etc/newrelic-infra.yml"; // FIXME CHANGE TO NRDOT
const NEWRELIC_INFRA_ARGS: [&str; 2] = ["--config", NEWRELIC_INFRA_CONFIG_PATH]; // FIXME CHANGE TO NRDOT

pub struct NRDOTConfig {
    ctx: SupervisorContext,
    snd: Sender<Event>,
    cfg: Option<AgentConfig>,
}

impl From<&NRDOTConfig> for SupervisorRunner {
    fn from(value: &NRDOTConfig) -> Self {
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

impl NRDOTConfig {
    pub fn new(ctx: SupervisorContext, snd: Sender<Event>, cfg: Option<AgentConfig>) -> Self {
        NRDOTConfig { ctx, snd, cfg }
    }
}
