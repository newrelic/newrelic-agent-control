use std::sync::mpsc::Sender;

use crate::{command::stream::Event, config::agent_configs::AgentConfig, context::Context};

use super::{restart::BackoffStrategy, runner::SupervisorRunner};

pub struct Config {
    ctx: Context<bool>,
    bin: String,
    args: Vec<String>,
    env: Vec<String>,
    snd: Sender<Event>,
    cfg: AgentConfig,
}

impl From<&Config> for SupervisorRunner {
    fn from(config: &Config) -> Self {
        SupervisorRunner::new(
            config.ctx.clone(),
            config.bin.clone(),
            config.args.iter().map(|s| s.to_owned()).collect(),
            config.env.iter().map(|s| s.to_owned()).collect(),
            config.snd.clone(),
        )
        .with_restart_policy(
            config.cfg.restart_policy.restart_exit_codes.clone(),
            BackoffStrategy::from(&config.cfg.restart_policy.backoff_strategy),
        )
    }
}

impl Config {
    pub fn new(bin: String, args: String, env: String, snd: Sender<Event>, cfg: AgentConfig) -> Self {
        let v_args: Vec<String> = args.split(' ').map(|s| s.to_string()).collect();
        let v_env: Vec<String> = env.split(' ').map(|s| s.to_string()).collect();
        Config {
            ctx: Context::new(),
            bin,
            args: v_args,
            env: v_env,
            snd,
            cfg,
        }
    }
}
