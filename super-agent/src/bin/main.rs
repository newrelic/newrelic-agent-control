use newrelic_super_agent::agent_type::agent_type_registry::AgentRepositoryError;
use newrelic_super_agent::cli::{Cli, CliCommand, SuperAgentCliConfig};
use newrelic_super_agent::logging::config::FileLoggerGuard;
use newrelic_super_agent::sub_agent::effective_agents_assembler::EffectiveAgentsAssemblerError;
use newrelic_super_agent::super_agent::run::SuperAgentRunner;
use std::error::Error;
use std::process::exit;
use tracing::{error, info};

#[cfg(all(feature = "onhost", feature = "k8s", not(feature = "ci")))]
compile_error!("Feature \"onhost\" and feature \"k8s\" cannot be enabled at the same time");

#[cfg(all(not(feature = "onhost"), not(feature = "k8s")))]
compile_error!("Either feature \"onhost\" or feature \"k8s\" must be enabled");

fn main() {
    let cli_command = Cli::init().unwrap_or_else(|cli_error| {
        println!("Error parsing CLI arguments: {}", cli_error);
        exit(1);
    });

    let super_agent_config = match cli_command {
        // Super Agent command call instructs normal operation. Continue with required data.
        CliCommand::InitSuperAgent(cli) => cli,

        // Super Agent command call was a "one-shot" operation. Exit successfully after performing.
        CliCommand::OneShot(op) => {
            op.run_one_shot();
            exit(0);
        }
    };

    if let Err(e) = _main(super_agent_config) {
        error!(
            "The super agent main process exited with an error: {}",
            e.to_string()
        );
        exit(1);
    }
}

// This function is the actual main function, but it is separated from the main function to allow
// propagating the errors and log them in a string format avoiding logging the error message twice.
// If we propagate the error to the main function, the error is logged in string format and
// in "Rust mode"
// i.e.
// Could not read Super Agent config from /invalid/path: error loading the super agent config: `error retrieving config: `missing field `agents```
// Error: ConfigRead(LoadConfigError(ConfigError(missing field `agents`)))
fn _main(super_agent_config: SuperAgentCliConfig) -> Result<(), Box<dyn Error>> {
    // Acquire the file logger guard (if any) for the whole duration of the program
    // Needed for remaining usages of `tracing` macros in `main`.
    let _guard: FileLoggerGuard = super_agent_config.file_logger_guard;

    #[cfg(all(unix, feature = "onhost"))]
    if !nix::unistd::Uid::effective().is_root() {
        error!("Program must run as root");
        std::process::exit(1);
    }

    // Pass the rest of required configs to the actual super agent runner
    SuperAgentRunner::try_from(super_agent_config.run_config)?.run()?;

    info!("exiting gracefully");
    Ok(())
}
