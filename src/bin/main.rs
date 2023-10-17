use std::error::Error;

use newrelic_super_agent::agent::effective_agents_assembler::{
    EffectiveAgentsAssembler, LocalEffectiveAgentsAssembler,
};
use tracing::{error, info};

use newrelic_super_agent::agent::instance_id::ULIDInstanceIDGetter;
use newrelic_super_agent::config::loader::{SuperAgentConfigLoader, SuperAgentConfigLoaderFile};
use newrelic_super_agent::opamp::client_builder::OpAMPHttpBuilder;
use newrelic_super_agent::{
    agent::{Agent, AgentEvent},
    cli::Cli,
    context::Context,
    logging::Logging,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // init logging singleton
    Logging::try_init()?;

    let cli = Cli::init_super_agent_cli();

    if cli.print_debug_info() {
        println!("Printing debug info");
        println!("CLI: {:#?}", cli);
        return Ok(());
    }

    // Program must run as root, but should accept simple behaviors such as --version, --help, etc
    #[cfg(unix)]
    if !nix::unistd::Uid::effective().is_root() {
        return Err("Program must run as root".into());
    }

    info!("Creating the global context");
    let ctx: Context<Option<AgentEvent>> = Context::new();

    info!("Creating the signal handler");
    ctrlc::set_handler({
        let ctx = ctx.clone();
        move || ctx.cancel_all(Some(AgentEvent::Stop)).unwrap()
    })
    .map_err(|e| {
        error!("Could not set signal handler: {}", e);
        e
    })?;

    // load effective config
    let super_agent_config =
        SuperAgentConfigLoaderFile::new(&cli.get_config_path()).load_config()?;

    let opamp_client_builder = super_agent_config
        .opamp
        .as_ref()
        .map(|opamp_config| OpAMPHttpBuilder::new(opamp_config.clone()));

    let instance_id_getter = ULIDInstanceIDGetter::default();

    let effective_agents =
        LocalEffectiveAgentsAssembler::default().assemble_agents(&super_agent_config)?;

    info!("Starting the super agent");
    Ok(Agent::new(
        super_agent_config,
        effective_agents,
        opamp_client_builder,
        instance_id_getter,
    )
    .run(ctx)?)
}
