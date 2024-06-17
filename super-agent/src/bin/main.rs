use newrelic_super_agent::cli::{Cli, CliCommand};
use newrelic_super_agent::logging::config::FileLoggerGuard;
use std::error::Error;
use tracing::{error, info};

#[cfg(all(feature = "onhost", feature = "k8s", not(feature = "ci")))]
compile_error!("Feature \"onhost\" and feature \"k8s\" cannot be enabled at the same time");

#[cfg(all(not(feature = "onhost"), not(feature = "k8s")))]
compile_error!("Either feature \"onhost\" or feature \"k8s\" must be enabled");

fn main() -> Result<(), Box<dyn Error>> {
    // Get the action requested from the command call
    let super_agent_config = match Cli::init()? {
        // Super Agent command call instructs normal operation. Continue with required data.
        CliCommand::InitSuperAgent(cli) => cli,
        // Super Agent command call was an "one-shot" operation. Exit successfully after performing.
        CliCommand::Quit(op) => return Ok(op.run_one_shot()?),
    };

    // Acquire the file logger guard (if any) for the whole duration of the program
    // Needed for remaining usages of `tracing` macros in `main`.
    let _guard: FileLoggerGuard = super_agent_config.file_logger_guard;

    #[cfg(all(unix, feature = "onhost"))]
    if !nix::unistd::Uid::effective().is_root() {
        error!("Program must run as root");
        std::process::exit(1);
    }

    // Pass the rest of required configs to the actual super agent runner
    super_agent_config
        .run_config
        .init()?
        .run()
        .inspect_err(|err| {
            error!(
                "The super agent main process exited with an error: {}",
                err.to_string()
            )
        })?;

    info!("exiting gracefully");
    Ok(())
}
