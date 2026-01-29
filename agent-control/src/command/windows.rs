//! This module contains functions to handle the Windows version of the main, which involves a Windows Service
//! running mode.

use crate::event::ApplicationEvent;
use crate::event::channel::EventPublisher;
use std::error::Error;
use std::sync::OnceLock;
use tracing::error;
use windows_service::{
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle},
};

/// Global handle used by the event handler to signal state changes (like StopPending) to Windows.
/// This allows the closure to access the handle, which is only available after registration.
pub static GLOBAL_SERVICE_HANDLE: OnceLock<ServiceStatusHandle> = OnceLock::new();

/// Defines the name for the Windows Service.
pub const WINDOWS_SERVICE_NAME: &str = "newrelic-agent-control";

/// Type alias to simplify [setup_windows_service] definition.
type WinServiceResult = Result<(), Box<dyn Error>>;

/// A handler that ensures the service reports a "Stopped" status if the process panics.
pub struct PanicStatusHandler {
    handle: ServiceStatusHandle,
}

impl Drop for PanicStatusHandler {
    fn drop(&mut self) {
        // If we are dropping during a panic, report the stop to Windows
        if std::thread::panicking() {
            let mut status = ServiceStatus::from(WindowsServiceStatus::Stopped);
            // Non-zero exit code indicates an abnormal termination
            status.exit_code = ServiceExitCode::Win32(1);

            let _ = self.handle.set_service_status(status).inspect_err(|e| {
                error!("Failed to set stopped status during panic: {e}");
            });
        }
    }
}

/// Type alias for the Windows service teardown closure to reduce type complexity.
pub type WinServiceTeardown = Box<dyn Fn(Result<(), &Box<dyn Error>>) -> WinServiceResult>;

/// WinServiceSetup contains the function to call teardown and the panic handler
pub struct WinServiceSetup {
    /// A guard that ensures the service reports a stopped state to Windows if the process panics.
    pub panic_handler: PanicStatusHandler,
    /// A closure called at the end of execution to transition the service to the stopped state.
    pub teardown: WinServiceTeardown,
}

/// Sets up the Windows Service by creating the status handler and setting the service status as [WindowsServiceStatus::Running].
/// It returns a function to tear the service down when the Agent Control finishes its execution
/// and a PanicHandler that will communicate the service that is stopped if the thead is panicking.
pub fn setup_windows_service(
    application_event_publisher: EventPublisher<ApplicationEvent>,
) -> Result<WinServiceSetup, Box<dyn Error>> {
    let windows_status_handler = service_control_handler::register(
        WINDOWS_SERVICE_NAME,
        windows_event_handler(application_event_publisher),
    )?;

    // Store the handle globally so it can be accessed from inside the windows_event_handler
    let _ = GLOBAL_SERVICE_HANDLE.set(windows_status_handler);

    windows_status_handler.set_service_status(WindowsServiceStatus::Running.into())?;

    // Create the panic handler by cloning the handle
    let panic_handler = PanicStatusHandler {
        handle: windows_status_handler,
    };

    // Return both the guard and the teardown closure
    let teardown = move |run_result: Result<(), &Box<dyn Error>>| {
        let mut status = ServiceStatus::from(WindowsServiceStatus::Stopped);

        if let Err(err) = run_result {
            error!("Service stopping due to error: {err}");
            status.exit_code = ServiceExitCode::ServiceSpecific(1);
        }

        windows_status_handler.set_service_status(status)?;
        Ok(())
    };

    Ok(WinServiceSetup {
        panic_handler,
        teardown: Box::new(teardown),
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
                // Eliminates the "Unresponsive" providing immediate feedback.
                if let Some(handle) = GLOBAL_SERVICE_HANDLE.get() {
                    let _ = handle.set_service_status(WindowsServiceStatus::StopPending.into());
                }

                let _ = publisher
                    .publish(ApplicationEvent::StopRequested)
                    .inspect_err(|err| error!("Could not send agent control stop request {err}"));
                ServiceControlHandlerResult::NoError
            }
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
    /// Represents that the service is pending to stop
    StopPending,
}

impl From<WindowsServiceStatus> for ServiceStatus {
    fn from(value: WindowsServiceStatus) -> Self {
        let (current_state, controls_accepted, wait_hint) = match value {
            WindowsServiceStatus::Running => (
                ServiceState::Running,
                ServiceControlAccept::STOP,
                std::time::Duration::default(),
            ),
            WindowsServiceStatus::StopPending => (
                ServiceState::StopPending,
                ServiceControlAccept::empty(),
                std::time::Duration::from_secs(10), // Tells Windows to wait for cleanup
            ),
            WindowsServiceStatus::Stopped => (
                ServiceState::Stopped,
                ServiceControlAccept::empty(),
                std::time::Duration::default(),
            ),
        };

        ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state,
            controls_accepted,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint,
            process_id: None,
        }
    }
}
