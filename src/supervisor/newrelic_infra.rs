use std::sync::mpsc::Sender;

use crate::command::stream::OutputEvent;

use super::{context::SupervisorContext, runner::SupervisorRunner};

const NEWRELIC_INFRA_PATH: &str = "/usr/bin/newrelic-infra";
const NEWRELIC_INFRA_CONFIG_PATH: &str = "/etc/newrelic-infra.yml";
const NEWRELIC_INFRA_ARGS: [&str; 2] = ["--config", NEWRELIC_INFRA_CONFIG_PATH];

pub struct NRIConfig {
    pub tx: Sender<OutputEvent>,
}

impl From<&NRIConfig> for SupervisorRunner {
    fn from(value: &NRIConfig) -> Self {
        SupervisorRunner::new(
            NEWRELIC_INFRA_PATH.to_owned(),
            NEWRELIC_INFRA_ARGS.iter().map(|&s| s.to_owned()).collect(),
            SupervisorContext::new(),
            value.tx.clone(),
        )
    }
}
