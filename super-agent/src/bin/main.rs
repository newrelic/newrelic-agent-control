use newrelic_super_agent::cli::{Cli, CliCommand, SuperAgentCliConfig};
use newrelic_super_agent::event::channel::{pub_sub, EventPublisher};
use newrelic_super_agent::event::ApplicationEvent;
use newrelic_super_agent::http::tls::install_rustls_default_crypto_provider;
use newrelic_super_agent::logging::config::FileLoggerGuard;
#[cfg(all(unix, feature = "onhost", not(feature = "multiple-instances")))]
use newrelic_super_agent::super_agent::pid_cache::PIDCache;
use newrelic_super_agent::super_agent::run::SuperAgentRunner;
use std::error::Error;
use std::process::exit;
use tracing::{error, info, trace};

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
        exit(1);
    }

    #[cfg(all(unix, feature = "onhost", not(feature = "multiple-instances")))]
    if let Err(err) = PIDCache::default().store(std::process::id()) {
        error!(error_msg = %err, "Error saving main process id");
        exit(1);
    }

    install_rustls_default_crypto_provider();

    trace!("creating the global context");
    let (application_event_publisher, application_event_consumer) = pub_sub();

    trace!("creating the signal handler");
    create_shutdown_signal_handler(application_event_publisher)?;

    // Create the actual super agent runner with the rest of required configs and the application_event_consumer
    SuperAgentRunner::new(super_agent_config.run_config, application_event_consumer)?.run()?;

    info!("exiting gracefully");
    Ok(())
}

pub fn create_shutdown_signal_handler(
    publisher: EventPublisher<ApplicationEvent>,
) -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(move || {
        info!("Received SIGINT (Ctrl-C). Stopping super agent");
        let _ = publisher
            .publish(ApplicationEvent::StopRequested)
            .inspect_err(|e| error!("Could not send super agent stop request: {}", e));
    })
    .map_err(|e| {
        error!("Could not set signal handler: {}", e);
        e
    })?;

    Ok(())
}
