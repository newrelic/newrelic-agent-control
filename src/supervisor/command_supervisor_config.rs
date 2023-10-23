use std::collections::HashMap;

use std::sync::mpsc::Sender;

use crate::supervisor::restart_policy::RestartPolicy;
use crate::{command::stream::Event, context::Context};

#[derive(Debug, Clone)]
pub struct SupervisorConfigOnHost {
    pub(in crate::supervisor) ctx: Context<bool>,
    pub(in crate::supervisor) bin: String,
    pub(in crate::supervisor) args: Vec<String>,
    pub(in crate::supervisor) env: HashMap<String, String>,
    pub(in crate::supervisor) snd: Sender<Event>,
    pub(in crate::supervisor) restart_policy: RestartPolicy,
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
