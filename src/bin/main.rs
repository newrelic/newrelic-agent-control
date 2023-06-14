use std::error::Error;

use meta_agent::{agent::Agent, cli::Cli};

fn main() -> Result<(), Box<dyn Error>> {
    println!("Starting the meta agent");
    let cli = Cli::init_meta_agent_cli();

    if cli.print_debug_info() {
        println!("Printing debug info");
        println!("CLI: {:#?}", cli);
        println!("CFG: {:#?}", cli.get_config_path());
        return Ok(());
    }

    Agent::new(&cli.get_config_path())?.run()?;
    Ok(())
}
