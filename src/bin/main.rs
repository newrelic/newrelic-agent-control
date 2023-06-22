use newrelic_super_agent::{
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

    let cli = Cli::init_super_agent_cli();

    if cli.print_debug_info() {
        println!("Printing debug info");
        println!("CLI: {:#?}", cli);
        println!("CFG: {:#?}", cli.get_config_path());
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

    info!("Starting the super agent");
    Ok(Agent::new(&cli.get_config_path())?.run(ctx)?)
}
