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
use newrelic_agent_control::rollback_probation::retrieve_rollback_probation_data;
use newrelic_agent_control::utils::is_elevated::is_elevated;
use std::error::Error;
use std::process::ExitCode;
use tracing::{error, info, trace};

#[cfg(target_os = "windows")]
use newrelic_agent_control::command::windows::{WINDOWS_SERVICE_NAME, setup_windows_service};

#[cfg(target_os = "windows")]
windows_service::define_windows_service!(ffi_service_main, service_main);

fn main() -> ExitCode {
    // Let's check for rollback
    let maybe_boot_data = retrieve_rollback_probation_data();
    if maybe_boot_data.is_none() {
        info!("No previous boot data found.");
    }
    let boot_data = maybe_boot_data.unwrap_or_default();
    info!(
        "Current boot data: status={:?}, previous_version={:?}, backup_path={:?}, n_attempts={}",
        boot_data.status(),
        boot_data.previous_version(),
        boot_data.backup_path(),
        boot_data.n_attempts()
    );

    if boot_data.should_trigger_rollback() {
        // Restore files and exit
        todo!();
    }

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
    // panic!("Making Windows service fail to test reboots");

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
    trace!("creating the global context");
    let (application_event_publisher, application_event_consumer) = pub_sub();

    #[cfg(target_os = "windows")]
    let stop_handler = as_windows_service
        .then(|| setup_windows_service(application_event_publisher.clone()))
        .transpose()?;

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

    trace!("creating the signal handler");
    create_shutdown_signal_handler(application_event_publisher)?;

    // Create the actual agent control runner with the rest of required configs
    // and the application_event_consumer and capture the result to report the error in windows
    let run_result = AgentControlRunner::new(agent_control_run_config, application_event_consumer)
        .and_then(|runner| Ok(runner.run()?));

    #[cfg(target_os = "windows")]
    if let Some(handler) = stop_handler {
        // Teardown notifies Windows that we're stopping intentionally, avoiding a 1061 state.
        // 1061 occurs in Windows when a service is busy, unresponsive, or experiencing a conflict,
        // preventing it from starting, stopping, or restarting.
        if let Err(e) = handler.teardown(&run_result) {
            error!("Failed to report service stop to Windows: {e}");
        }
    }

    run_result
        .inspect_err(|e| error!("Agent Control Runner failed: {e}"))
        .inspect(|_| info!("Exiting gracefully"))
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
