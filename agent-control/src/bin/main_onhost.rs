//! This is the entry point for the on-host implementation of Agent Control.
//!
//! It implements the basic functionality of parsing the command line arguments and either
//! performing one-shot actions or starting the main agent control process.
#![warn(missing_docs)]

#[cfg(all(unix, not(feature = "multiple-instances")))]
use newrelic_agent_control::agent_control::pid_cache::PIDCache;
use newrelic_agent_control::agent_control::run::{AgentControlRunner, Environment};
use newrelic_agent_control::cli::{AgentControlCliConfig, Cli, CliCommand};
use newrelic_agent_control::event::channel::{pub_sub, EventPublisher};
use newrelic_agent_control::event::ApplicationEvent;
use newrelic_agent_control::http::tls::install_rustls_default_crypto_provider;
use newrelic_agent_control::instrumentation::tracing::TracingGuardBox;
use std::error::Error;
use std::process::ExitCode;
use tracing::{error, info, trace};

const AGENT_CONTROL_MODE: Environment = Environment::OnHost;

fn main() -> ExitCode {
    let Ok(cli_command) = Cli::init(AGENT_CONTROL_MODE)
        .inspect_err(|cli_err| println!("Error parsing CLI arguments: {}", cli_err))
    else {
        return ExitCode::FAILURE;
    };

    let (agent_control_config, tracer) = match cli_command {
        // Agent Control command call instructs normal operation. Continue with required data.
        CliCommand::InitAgentControl(cli, tracer) => (cli, tracer),

        // Agent Control command call was a "one-shot" operation. Exit successfully after performing.
        CliCommand::OneShot(op) => {
            op.run_one_shot();
            return ExitCode::SUCCESS;
        }
    };

    match _main(agent_control_config, tracer) {
        Err(e) => {
            error!("The agent control main process exited with an error: {e}");
            ExitCode::FAILURE
        }
        Ok(()) => {
            info!("The agent control main process exited successfully");
            ExitCode::SUCCESS
        }
    }
}

/// This is the actual main function.
///
/// It is separated from [main] to allow propagating
/// the errors and log them in a string format, avoiding logging the error message twice.
/// If we just propagate the error to the main function, the error is logged in string format and
/// in "Rust mode", i.e. like this:
/// ```sh
/// Could not read Agent Control config from /invalid/path: error loading the agent control config: \`error retrieving config: \`missing field \`agents\`\`\`
/// Error: ConfigRead(LoadConfigError(ConfigError(missing field \`agents\`)))
/// ```
fn _main(
    agent_control_config: AgentControlCliConfig,
    _tracer: Vec<TracingGuardBox>, // Needs to take ownership of the tracer as it can be shutdown on drop
) -> Result<(), Box<dyn Error>> {
    #[cfg(unix)]
    if !nix::unistd::Uid::effective().is_root() {
        return Err("Program must run as root".into());
    }

    #[cfg(all(unix, not(feature = "multiple-instances")))]
    if let Err(err) = PIDCache::default().store(std::process::id()) {
        return Err(format!("Error saving main process id: {}", err).into());
    }

    install_rustls_default_crypto_provider();

    trace!("creating the global context");
    let (application_event_publisher, application_event_consumer) = pub_sub();

    trace!("creating the signal handler");
    create_shutdown_signal_handler(application_event_publisher)?;

    // Create the actual agent control runner with the rest of required configs and the application_event_consumer
    AgentControlRunner::new(agent_control_config.run_config, application_event_consumer)?
        .run(AGENT_CONTROL_MODE)?;

    info!("exiting gracefully");

    Ok(())
}

/// Enables using the typical keypress (Ctrl-C) to stop the agent control process at any moment.
///
/// This means sending [ApplicationEvent::StopRequested] to the agent control event processor
/// so it can release all resources.
pub fn create_shutdown_signal_handler(
    publisher: EventPublisher<ApplicationEvent>,
) -> Result<(), ctrlc::Error> {
    ctrlc::set_handler(move || {
        info!("Received SIGINT (Ctrl-C). Stopping agent control");
        let _ = publisher
            .publish(ApplicationEvent::StopRequested)
            .inspect_err(|e| error!("Could not send agent control stop request: {}", e));
    })
    .map_err(|e| {
        error!("Could not set signal handler: {}", e);
        e
    })?;

    Ok(())
}
