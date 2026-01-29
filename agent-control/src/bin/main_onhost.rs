//! This is the entry point for the on-host implementation of Agent Control.
//!
//! It implements the basic functionality of parsing the command line arguments and either
//! performing one-shot actions or starting the main agent control process.
#![warn(missing_docs)]
use newrelic_agent_control::agent_control::run::on_host::AGENT_CONTROL_MODE_ON_HOST;
use newrelic_agent_control::agent_control::run::{AgentControlRunConfig, AgentControlRunner};
use newrelic_agent_control::command::Command;
use newrelic_agent_control::event::ApplicationEvent;
use newrelic_agent_control::event::channel::{EventPublisher, pub_sub};
use newrelic_agent_control::http::tls::install_rustls_default_crypto_provider;
use newrelic_agent_control::instrumentation::tracing::TracingGuardBox;
use newrelic_agent_control::utils::is_elevated::is_elevated;
use std::error::Error;
use std::process::ExitCode;
use tracing::{debug, error, info, trace};

#[cfg(target_os = "windows")]
use newrelic_agent_control::command::windows::{WINDOWS_SERVICE_NAME, setup_windows_service};

#[cfg(target_os = "windows")]
windows_service::define_windows_service!(ffi_service_main, service_main);

fn main() -> ExitCode {
    #[cfg(target_family = "unix")]
    {
        Command::run(AGENT_CONTROL_MODE_ON_HOST, _main)
    }

    #[cfg(target_os = "windows")]
    {
        if windows_service::service_dispatcher::start(WINDOWS_SERVICE_NAME, ffi_service_main)
            .is_err()
        {
            // Not running as Windows Service, run normally
            return Command::run(AGENT_CONTROL_MODE_ON_HOST, |cfg, tracer| {
                _main(cfg, tracer, false)
            });
        }
        ExitCode::SUCCESS
    }
}

#[cfg(target_os = "windows")]
/// Entry-point for Windows Service
fn service_main(_arguments: Vec<std::ffi::OsString>) {
    let _ = Command::run(AGENT_CONTROL_MODE_ON_HOST, |cfg, tracer| {
        _main(cfg, tracer, true)
    });
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
    #[cfg(target_os = "windows")] as_windows_service: bool,
) -> Result<(), Box<dyn Error>> {
    #[cfg(not(feature = "disable-asroot"))]
    if !is_elevated()? {
        return Err("Program must run with elevated permissions".into());
    }

    #[cfg(all(target_family = "unix", not(feature = "multiple-instances")))]
    if let Err(err) = newrelic_agent_control::agent_control::pid_cache::PIDCache::default()
        .store(std::process::id())
    {
        return Err(format!("Error saving main process id: {err}").into());
    }

    install_rustls_default_crypto_provider();

    trace!("creating the global context");
    let (application_event_publisher, application_event_consumer) = pub_sub();

    trace!("creating the signal handler");
    create_shutdown_signal_handler(application_event_publisher.clone())?;

    #[cfg(target_os = "windows")]
    let tear_down_windows_service = as_windows_service
        .then(|| setup_windows_service(application_event_publisher))
        .transpose()?;

    // Create the actual agent control runner with the rest of required configs
    // and the application_event_consumer and capture the result to report the error in windows
    let run_result = AgentControlRunner::new(agent_control_run_config, application_event_consumer)
        .and_then(|runner| runner.run().map_err(Box::from));

    #[cfg(target_os = "windows")]
    if let Some(tear_down_fn) = tear_down_windows_service {
        // We call this even if run_result is Err to clear the 1061 state
        if let Err(e) = tear_down_fn(run_result.as_ref().copied()) {
            error!("Failed to report service stop to Windows: {e}");
        }
    }

    if let Err(ref e) = run_result {
        error!("Agent Control Runner failed: {e}");
    } else {
        info!("Exiting gracefully");
    }

    run_result
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
