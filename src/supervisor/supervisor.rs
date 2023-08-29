use std::collections::HashMap;
use std::hash::Hash;
use std::sync::mpsc::Sender;

use crate::{command::stream::Event,
            config::agent_configs::{
                AgentConfig,
                RestartPolicyConfig,
            },
            context::Context,
};

use super::{restart::BackoffStrategy, runner::SupervisorRunner};

pub struct Config {
    ctx: Context<bool>,
    bin: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    snd: Sender<Event>,
    restart: RestartPolicyConfig,
}

impl From<&Config> for SupervisorRunner {
    fn from(config: &Config) -> Self {
        SupervisorRunner::new(
            config.ctx.clone(),
            config.bin.clone(),
            config.args.iter().map(|s| s.to_owned()).collect(),
            config.env.clone().into_iter().map(|s| s.to_owned()).collect(),
            config.snd.clone(),
        )
        .with_restart_policy(
            config.restart.restart_exit_codes.clone(),
            BackoffStrategy::from(&config.restart.backoff_strategy),
        )
    }
}

impl Config {
    pub fn new(bin: String, args: Vec<String>, env: HashMap<String, String>, snd: Sender<Event>, cfg: AgentConfig) -> Self {
        Config {
            ctx: Context::new(),
            bin,
            args,
            env,
            snd,
            restart: cfg.restart_policy,
        }
    }
}
