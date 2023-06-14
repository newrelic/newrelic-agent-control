use meta_agent::{agent::Agent, cli::Cli, logging::Logging};
use std::error::Error;
use tracing::info;

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

    info!("Starting the meta agent");

    Agent::new(&cli.get_config_path())?.run()?;
    Ok(())
}
