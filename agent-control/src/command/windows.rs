//! This module contains functions to handle the Windows version of the main, which involves a Windows Service
//! running mode.

use crate::event::channel::EventPublisher;
use crate::{event::ApplicationEvent, utils::retry::retry};
use std::error::Error;
use std::sync::OnceLock;
use std::time::Duration;
use tracing::error;
use windows_service::{
    service::{
        ServiceAccess, ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState,
        ServiceStatus, ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle},
    service_manager::{ServiceManager, ServiceManagerAccess},
};

/// Global handle used by the event handler to signal state changes (like StopPending) to Windows.
/// This allows the closure to access the handle, which is only available after registration.
pub static WINDOWS_SERVICE_HANDLE: OnceLock<ServiceStatusHandle> = OnceLock::new();

/// Defines the name for the Windows Service.
pub const WINDOWS_SERVICE_NAME: &str = "newrelic-agent-control";

const SCM_CHECK_MAX_ATTEMPTS: usize = 10;
const SCM_CHECK_INTERVALS: Duration = Duration::from_millis(200);

/// Manages the Windows Service lifecycle, ensuring the service status is updated on exit.
pub struct WindowsServiceStopHandler {
    handle: Option<ServiceStatusHandle>,
}

impl WindowsServiceStopHandler {
    /// Creates the StopHandler with the completed status to false.
    pub fn new(handle: ServiceStatusHandle) -> Self {
        Self {
            handle: Some(handle),
        }
    }

    /// Transitions the service to the Stopped state.
    /// Consumes the handler so that [Drop] is not triggered for panic/error logic.
    pub fn teardown(
        mut self,
        run_result: &Result<(), Box<dyn Error>>,
    ) -> Result<(), Box<dyn Error>> {
        if let Some(handle) = self.handle.take() {
            let exit_code = match run_result {
                Err(err) => {
                    error!("Service stopping due to error: {err}");
                    ServiceExitCode::ServiceSpecific(1)
                }
                Ok(_) => ServiceExitCode::Win32(0),
            };

            set_service_as_stopped(handle, exit_code)?;
        }
        Ok(())
    }
}

impl Drop for WindowsServiceStopHandler {
    fn drop(&mut self) {
        // If the handle is still Some, teardown wasn't called (Panic or early return)
        if let Some(handle) = self.handle.take() {
            // Win32(1) indicates an abnormal process termination to Windows
            let _ = set_service_as_stopped(handle, ServiceExitCode::Win32(1)).inspect_err(|e| {
                error!("Failed to report stopped status during abnormal exit: {e}");
            });
        }
    }
}

/// Helper to set the status server as stopped.
/// Before setting up the Stopped status it queries the SCM to check that the service was effectively registered
/// as running/stop-pending before. This delay avoids race-conditions if the service finishes right after starting.
/// If the status is not correctly checked after [SCM_CHECK_MAX_ATTEMPTS], the service is still set as stopped.
fn set_service_as_stopped(
    handle: ServiceStatusHandle,
    exit_code: ServiceExitCode,
) -> windows_service::Result<()> {
    let _ = retry(SCM_CHECK_MAX_ATTEMPTS, SCM_CHECK_INTERVALS, || {
        scm_service_ready_to_stop(WINDOWS_SERVICE_NAME)
    }).inspect_err(|err| {
        error!("Could not assure that '{WINDOWS_SERVICE_NAME}' service was in the expected state after {SCM_CHECK_MAX_ATTEMPTS} attempts: {err}");
    });
    let service_status = ServiceStatus {
        exit_code,
        ..ServiceStatus::from(WindowsServiceStatus::Stopped)
    };
    handle.set_service_status(service_status)
}

/// Sets up the Windows Service by creating the status handler and setting the service status as [WindowsServiceStatus::Running].
/// It returns the WindowsServiceStopHandler to tear the service down when the Agent Control finishes
/// its execution or communicate the stop if a Panic or an abnormal exit happens.
pub fn setup_windows_service(
    application_event_publisher: EventPublisher<ApplicationEvent>,
) -> Result<WindowsServiceStopHandler, Box<dyn Error>> {
    let handle = service_control_handler::register(
        WINDOWS_SERVICE_NAME,
        windows_event_handler(application_event_publisher),
    )?;

    let _ = WINDOWS_SERVICE_HANDLE.set(handle);
    handle.set_service_status(WindowsServiceStatus::Running.into())?;

    Ok(WindowsServiceStopHandler::new(handle))
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
                // Eliminates the "Unresponsive" error providing immediate feedback, passing the
                // status StopPending back to Windows so it knows stop is in process and needs
                // to wait the graceful period we specify (10 seconds).
                // This handler can't listen to the event StopPending that is only meant
                // to be emitted from a running service back to Windows ServiceControl.
                if let Some(handle) = WINDOWS_SERVICE_HANDLE.get() {
                    let _ = handle
                        .set_service_status(WindowsServiceStatus::StopPending.into())
                        .inspect_err(|e| error!("Failed to set status to StopPending: {e}"));
                }

                let _ = publisher
                    .publish(ApplicationEvent::StopRequested)
                    .inspect_err(|err| error!("Could not send agent control stop request: {err}"));

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

/// Queries the Windows Service Control Manager to check if the service with the given name exists and its current state
/// is [ServiceState::Running] or [ServiceState::StopPending].
pub fn scm_service_ready_to_stop(service_name: &str) -> Result<(), Box<dyn Error>> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service(service_name, ServiceAccess::QUERY_STATUS)?;
    let status = service.query_status()?;

    match status.current_state {
        ServiceState::Running | ServiceState::StopPending => Ok(()),
        _ => Err(format!(
            "service '{}' is not in Running or StopPending state (current state: {:?})",
            service_name, status.current_state
        )
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "eventlog" (Windows Event Log) is guaranteed to be running on Windows machines.
    const ALWAYS_RUNNING_SERVICE: &str = "eventlog";
    const NONEXISTING_SERVICE: &str = "no-service-with-this-name";

    #[test]
    fn test_check_scm_service_is_running() {
        assert!(scm_service_ready_to_stop(ALWAYS_RUNNING_SERVICE).is_ok());
        assert!(scm_service_ready_to_stop(NONEXISTING_SERVICE).is_err());
    }
}
