use std::sync::mpsc::{Receiver, Sender};

use log::info;

use crate::{
    agent::logging::Logging,
    cli::Cli,
    command::stream::Event,
    config::{agent_configs::MetaAgentConfig, resolver::Resolver},
    supervisor::context::SupervisorContext,
};

use super::error::AgentError;

pub struct Initializer {
    cli: Cli,
    cfg: MetaAgentConfig,
    chn: Option<(Sender<Event>, Receiver<Event>)>,
    ctx: SupervisorContext,
}

impl Initializer {
    pub fn get_commandline(&self) -> &Cli {
        &self.cli
    }

    pub fn get_configs(&self) -> &MetaAgentConfig {
        &self.cfg
    }

    pub fn extract_channel(&mut self) -> Result<(Sender<Event>, Receiver<Event>), AgentError> {
        self.chn.take().ok_or(AgentError::ChannelExtractError)
    }

    pub fn get_context(&self) -> SupervisorContext {
        self.ctx.clone()
    }
}

pub struct Lifecycle;

impl Lifecycle {
    pub fn init() -> Result<Initializer, Box<dyn std::error::Error>> {
        // Initial setup phase
        info!("Starting the meta agent");
        let cli = Cli::init_meta_agent_cli();
        let cfg = Resolver::retrieve_config(&cli.get_config())?;

        if cli.print_debug_info() {
            info!("Printing debug info");
            println!("CLI: {:#?}", cli);
            println!("CFG: {:#?}", cfg);
            Err(AgentError::Debug)?
        }

        Logging::init()?;

        info!("Creating communication channels");
        let chn = std::sync::mpsc::channel();

        info!("Creating the supervisor context");
        let ctx = SupervisorContext::new();

        Ok(Initializer {
            cli,
            cfg,
            chn: Some(chn),
            ctx,
        })
    }
}
