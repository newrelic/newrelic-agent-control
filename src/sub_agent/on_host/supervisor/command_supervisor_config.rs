use std::collections::HashMap;

use std::sync::mpsc::Sender;

use super::restart_policy::RestartPolicy;
use crate::{context::Context, sub_agent::on_host::command::stream::Event};

#[derive(Debug, Clone)]
pub struct SupervisorConfigOnHost {
    pub(super) ctx: Context<bool>,
    pub(super) bin: String,
    pub(super) args: Vec<String>,
    pub(super) env: HashMap<String, String>,
    pub(super) snd: Sender<Event>,
    pub(super) restart_policy: RestartPolicy,
}

impl SupervisorConfigOnHost {
    pub fn new(
        bin: String,
        args: Vec<String>,
        ctx: Context<bool>,
        env: HashMap<String, String>,
        snd: Sender<Event>,
        restart_policy: RestartPolicy,
    ) -> Self {
        SupervisorConfigOnHost {
            ctx,
            bin,
            args,
            env,
            snd,
            restart_policy,
        }
    }
}
