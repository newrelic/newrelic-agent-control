use meta_agent::{
    agent::{Agent, AgentEvent},
    cli::Cli,
    context::Context,
    logging::Logging,
};
use std::error::Error;
use tracing::{error, info};

fn main() -> Result<(), Box<dyn Error>> {
    // init logging singleton
    Logging::try_init()?;

    let cli = Cli::init_meta_agent_cli();

    if cli.print_debug_info() {
        println!("Printing debug info");
        println!("CLI: {:#?}", cli);
        println!("CFG: {:#?}", cli.get_config_path());
        return Ok(());
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

    info!("Starting the meta agent");
    Ok(Agent::new(&cli.get_config_path())?.run(ctx)?)
}
