//! This is the entry point for the Kubernetes implementation of Agent Control.
//!
//! It implements the basic functionality of parsing the command line arguments and either
//! performing one-shot actions or starting the main agent control process.
#![warn(missing_docs)]

use newrelic_agent_control::agent_control::run::{
    AgentControlRunConfig, AgentControlRunner, Environment,
};
use newrelic_agent_control::event::ApplicationEvent;
use newrelic_agent_control::event::channel::{EventPublisher, pub_sub};
use newrelic_agent_control::flags::{Command, Flags};
use newrelic_agent_control::http::tls::install_rustls_default_crypto_provider;
use newrelic_agent_control::instrumentation::tracing::TracingGuardBox;
use std::error::Error;
use std::process::ExitCode;
use tracing::{error, info, trace};

const AGENT_CONTROL_MODE: Environment = Environment::K8s;

fn main() -> ExitCode {
    let Ok(command) = Flags::init(AGENT_CONTROL_MODE)
        .inspect_err(|init_err| println!("Error parsing Flags: {init_err}"))
    else {
        return ExitCode::FAILURE;
    };

    let (agent_control_config, tracer) = match command {
        // Agent Control command call instructs normal operation. Continue with required data.
        Command::InitAgentControl(agent_control_init_config, tracer) => {
            (agent_control_init_config, tracer)
        }

        // Agent Control command call was a "one-shot" operation. Exit successfully after performing.
        Command::OneShot(op) => {
            op.run_one_shot(AGENT_CONTROL_MODE);
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
/// could not read Agent Control config from /invalid/path: error loading the agent control config: \`error retrieving config: \`missing field \`agents\`\`\`
/// Error: ConfigRead(LoadConfigError(ConfigError(missing field \`agents\`)))
/// ```
fn _main(
    agent_control_run_config: AgentControlRunConfig,
    _tracer: Vec<TracingGuardBox>, // Needs to take ownership of the tracer as it can be shutdown on drop
) -> Result<(), Box<dyn Error>> {
    install_rustls_default_crypto_provider();

    trace!("creating the global context");
    let (application_event_publisher, application_event_consumer) = pub_sub();

    trace!("creating the signal handler");
    create_shutdown_signal_handler(application_event_publisher)?;

    // Create the actual agent control runner with the rest of required configs and the application_event_consumer
    AgentControlRunner::new(agent_control_run_config, application_event_consumer)?
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
