use meta_agent::{agent::Agent, cli::Cli, context::Context, logging::Logging};
use std::error::Error;
use tracing::info;
use std::{sync::mpsc, thread};

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

    println!("Creating the global context");
    let ctx = Context::new();

    let signal_manager = thread::spawn({
        let ctx = ctx.clone();
        move || {
            let (tx, rx) = mpsc::channel();

            ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel"))
                .expect("Error setting signal handler");

            // Wait for shutdown signal
            rx.recv().expect("Could not receive from signal channel.");
            println!("Graceful shutdown");
            ctx.cancel_all().unwrap();
        }
    });

    info!("Starting the meta agent");
    Agent::new(&cli.get_config_path())?.run(ctx.clone())?;

    // Ending the program
    println!("Waiting for the signal manager to finish");
    signal_manager.join().unwrap();

    Ok(())
}
