//! This module contains functions to handle the Windows version of the main, which involves a Windows Service
//! running mode.

use crate::event::ApplicationEvent;
use crate::event::channel::EventPublisher;
use std::error::Error;
use tracing::error;
use windows_service::{
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
};

/// Defines the name for the Windows Service.
pub const WINDOWS_SERVICE_NAME: &str = "newrelic-agent-control";

/// Type alias to simplify [setup_windows_service] definition.
type WinServiceResult = Result<(), Box<dyn Error>>;

/// Sets up the Windows Service by creating the status handler and setting the service status as [WindowsServiceStatus::Running].
/// It returns a function to tear the service down when the Agent Control finishes its execution.
pub fn setup_windows_service(
    application_event_publisher: EventPublisher<ApplicationEvent>,
) -> Result<impl Fn(Result<(), &Box<dyn Error>>) -> WinServiceResult, Box<dyn Error>> {
    let windows_status_handler = service_control_handler::register(
        WINDOWS_SERVICE_NAME,
        windows_event_handler(application_event_publisher),
    )?;
    windows_status_handler.set_service_status(WindowsServiceStatus::Running.into())?;

    Ok(move |run_result: Result<(), &Box<dyn Error>>| {
        let mut status = ServiceStatus::from(WindowsServiceStatus::Stopped);

        // If the runner failed, report a non-zero exit code to Windows
        if let Err(err) = run_result {
            error!("Service stopping due to error: {err}");
            status.exit_code = ServiceExitCode::Win32(1);
        }

        windows_status_handler.set_service_status(status)?;
        Ok(())
    })
}

/// Handles windows services events and stops the Agent Control if the specific events are received.
/// See the '[Service Control Handler Function](https://learn.microsoft.com/en-us/windows/win32/services/service-control-handler-function)'
/// page for details.
pub fn windows_event_handler(
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

/// Internal, simplified representation of [ServiceStatus]
pub enum WindowsServiceStatus {
    /// Represents that the service is running
    Running,
    /// Represents that the service is stopped
    Stopped,
}

impl From<WindowsServiceStatus> for ServiceStatus {
    fn from(value: WindowsServiceStatus) -> Self {
        let (current_state, controls_accepted) = match value {
            WindowsServiceStatus::Running => (ServiceState::Running, ServiceControlAccept::STOP),
            WindowsServiceStatus::Stopped => (ServiceState::Stopped, ServiceControlAccept::empty()),
        };
        ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state,
            controls_accepted,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: std::time::Duration::default(),
            process_id: None,
        }
    }
}
