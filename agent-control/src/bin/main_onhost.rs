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
use tracing::{error, info, trace};

#[cfg(target_os = "windows")]
use std::ffi::OsString;
#[cfg(target_os = "windows")]
use windows_service::service_control_handler::ServiceStatusHandle;
#[cfg(target_os = "windows")]
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

const AGENT_CONTROL_MODE: Environment = Environment::OnHost;

#[cfg(target_os = "windows")]
const WINDOWS_SERVICE_NAME: &str = "newrelic-agent-control";

#[cfg(target_os = "windows")]
define_windows_service!(ffi_service_main, service_main);

fn main() -> ExitCode {
    #[cfg(not(target_os = "windows"))]
    {
        Command::run(AGENT_CONTROL_MODE, _main)
    }

    #[cfg(target_os = "windows")]
    {
        if service_dispatcher::start(WINDOWS_SERVICE_NAME, ffi_service_main).is_err() {
            // Not running as service, run normally
            return Command::run(AGENT_CONTROL_MODE, _main);
        }
        ExitCode::SUCCESS
    }
}

#[cfg(target_os = "windows")]
fn service_main(_arguments: Vec<OsString>) {
    let _ = Command::run(AGENT_CONTROL_MODE, _windows_service_main);
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

    // Create the actual agent control runner with the rest of required configs and the application_event_consumer
    AgentControlRunner::new(agent_control_run_config, application_event_consumer)?.run()?;

    info!("Exiting gracefully");

    Ok(())
}

// TODO: avoid full duplication of _main
#[cfg(target_os = "windows")]
fn _windows_service_main(
    agent_control_run_config: AgentControlRunConfig,
    _tracer: Vec<TracingGuardBox>, // Needs to take ownership of the tracer as it can be shutdown on drop
) -> Result<(), Box<dyn Error>> {
    #[cfg(not(feature = "multiple-instances"))]
    if let Err(err) = PIDCache::default().store(std::process::id()) {
        return Err(format!("Error saving main process id: {err}").into());
    }

    install_rustls_default_crypto_provider();

    trace!("creating the global context");
    let (application_event_publisher, application_event_consumer) = pub_sub();

    trace!("creating the signal handler");
    create_shutdown_signal_handler(application_event_publisher.clone())?;

    let windows_status_handler = service_control_handler::register(
        WINDOWS_SERVICE_NAME,
        windows_event_handler(application_event_publisher),
    )?;
    set_windows_service_status(&windows_status_handler, WindowsServiceStatus::Running)?;

    // Create the actual agent control runner with the rest of required configs and the application_event_consumer
    AgentControlRunner::new(agent_control_run_config, application_event_consumer)?.run()?;

    // TODO: check if we should inform of stop-requested in case the graceful shutdown takes too long.
    set_windows_service_status(&windows_status_handler, WindowsServiceStatus::Stopped)?;

    info!("Exiting gracefully");

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

#[cfg(target_os = "windows")]
/// Handles windows services events and stops the Agent Control if the specific events are received.
/// See the '[Service Control Handler Function](https://learn.microsoft.com/en-us/windows/win32/services/service-control-handler-function)'
/// page for details.
fn windows_event_handler(
    publisher: EventPublisher<ApplicationEvent>,
) -> impl Fn(ServiceControl) -> ServiceControlHandlerResult {
    move |event: ServiceControl| -> ServiceControlHandlerResult {
        match event {
            ServiceControl::Stop => {
                let _ = publisher
                    .publish(ApplicationEvent::StopRequested)
                    .inspect_err(|err| error!("Could not send agent control stop request {err}"));
                ServiceControlHandlerResult::NoError
            }
            // Interrogate needs to return `NoError` even if it is a No-Op operation.
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    }
}

#[cfg(target_os = "windows")]
enum WindowsServiceStatus {
    Running,
    Stopped,
}

#[cfg(target_os = "windows")]
impl From<WindowsServiceStatus> for ServiceStatus {
    fn from(value: WindowsServiceStatus) -> Self {
        match value {
            WindowsServiceStatus::Running => ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Running,
                controls_accepted: ServiceControlAccept::STOP,
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: std::time::Duration::default(),
                process_id: None,
            },
            WindowsServiceStatus::Stopped => ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Stopped,
                controls_accepted: ServiceControlAccept::empty(),
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: std::time::Duration::default(),
                process_id: None,
            },
        }
    }
}

#[cfg(target_os = "windows")]
/// Helper to set the application service status for Windows services.
fn set_windows_service_status(
    status_handler: &ServiceStatusHandle,
    status: WindowsServiceStatus,
) -> windows_service::Result<()> {
    status_handler.set_service_status(status.into())
}
