//! This is the entry point for the Kubernetes implementation of Agent Control.
//!
//! It implements the basic functionality of parsing the command line arguments and either
//! performing one-shot actions or starting the main agent control process.
#![warn(missing_docs)]

use newrelic_agent_control::agent_control::run::AgentControlRunner;
use newrelic_agent_control::agent_control::run::k8s::AGENT_CONTROL_MODE_K8S;
use newrelic_agent_control::command::{Command, RunContext};
use newrelic_agent_control::event::ApplicationEvent;
use newrelic_agent_control::event::channel::EventPublisher;
use newrelic_agent_control::http::tls::install_rustls_default_crypto_provider;
use std::error::Error;
use std::process::ExitCode;
use tracing::{error, info, trace};

fn main() -> ExitCode {
    #[cfg(target_family = "unix")]
    return Command::run(AGENT_CONTROL_MODE_K8S, _main);
    #[cfg(target_family = "windows")]
    return Command::run(AGENT_CONTROL_MODE_K8S, _main, false);
}

/// This is the actual main function.
///
/// It is separated from [main] to allow propagating
/// the errors and log them in a string format, avoiding logging the error message twice.
/// If we just propagate the error to the main function, the error is logged in string format and
/// in "Rust mode", i.e. like this:
/// ```sh
/// could not read Agent Control config from /invalid/path: error loading the agent control config: \`error retrieving config: \`missing field \`agents\`\`\`
/// Error: ConfigRead(LoadConfigError(ConfigError(missing field \`agents\`)))
/// ```
fn _main(run_context: RunContext) -> Result<(), Box<dyn Error>> {
    install_rustls_default_crypto_provider();

    trace!("creating the signal handler");
    create_shutdown_signal_handler(run_context.application_event_publisher)?;

    // Create the actual agent control runner with the rest of required configs and the application_event_consumer
    AgentControlRunner::new(
        run_context.run_config,
        run_context.application_event_consumer,
    )?
    .run()?;

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
    .inspect_err(|e| error!("Could not set signal handler: {e}"))
}
