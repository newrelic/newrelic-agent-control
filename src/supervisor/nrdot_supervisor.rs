use std::sync::mpsc::Sender;

use crate::command::stream::Event;

use super::{context::SupervisorContext, runner::SupervisorRunner};

const NEWRELIC_INFRA_PATH: &str = "/usr/bin/newrelic-infra";
const NEWRELIC_INFRA_CONFIG_PATH: &str = "/etc/newrelic-infra.yml";
const NEWRELIC_INFRA_ARGS: [&str; 2] = ["--config", NEWRELIC_INFRA_CONFIG_PATH];

pub struct NRDOTConfig(pub SupervisorContext, pub Sender<Event>);

impl From<&NRDOTConfig> for SupervisorRunner {
    fn from(value: &NRDOTConfig) -> Self {
        SupervisorRunner::new(
            NEWRELIC_INFRA_PATH.to_owned(),
            NEWRELIC_INFRA_ARGS.iter().map(|&s| s.to_owned()).collect(),
            value.0.clone(),
            value.1.clone(),
        )
    }
}

impl NRDOTConfig {
    pub fn new(ctx: SupervisorContext, tx: Sender<Event>) -> Self {
        Self(ctx, tx)
    }
}
