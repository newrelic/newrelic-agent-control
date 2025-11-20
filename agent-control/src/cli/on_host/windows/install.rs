use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use crate::agent_control::defaults::{
    AGENT_CONTROL_DATA_DIR, AGENT_CONTROL_LOCAL_DATA_DIR, AGENT_CONTROL_LOG_DIR,
};
use crate::cli::error::CliError;
use crate::cli::on_host::config_gen;
use crate::utils::binary_metadata::VERSION;
use crate::utils::is_elevated::is_elevated;
use clap::{CommandFactory as _, FromArgMatches};
use tracing::info;

use std::ffi::OsString;
use windows_service::{
    service::{
        ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceState,
        ServiceType,
    },
    service_manager::{ServiceManager, ServiceManagerAccess},
};
use windows_sys::Win32::Foundation::ERROR_SERVICE_DOES_NOT_EXIST;

const SERVICE_NAME: &str = "newrelic-agent-control";
const SERVICE_DISPLAY_NAME: &str = "New Relic Agent Control";
const EXECUTABLE_NAME: &str = "newrelic-agent-control.exe";

pub fn install_agent_control_as_windows_service() -> Result<(), CliError> {
    let is_admin = is_elevated().map_err(|err| {
        CliError::Command(format!(
            "Could not check if the user has the proper permissions: {err}"
        ))
    })?;
    if !is_admin {
        return Err(CliError::Command(
            "Installation must run with elevated permissions".into(),
        ));
    }

    info!("Installing New Relic Agent Control {VERSION}...");

    stop_and_delete_previous_service_if_exists()?;

    let (ac_binary_dir, ac_config_dir) = setup_agent_control_directories()?;

    copy_agent_control_binary(&ac_binary_dir)?;

    generate_config(&ac_config_dir)?;

    install_service(ac_binary_dir.join(EXECUTABLE_NAME))?;

    info!("Installation completed!");

    Ok(())
}

fn setup_agent_control_directories() -> Result<(PathBuf, PathBuf), CliError> {
    let ac_binary_directory = PathBuf::from(AGENT_CONTROL_LOCAL_DATA_DIR);
    let ac_config_directory = PathBuf::from(AGENT_CONTROL_LOCAL_DATA_DIR)
        .join("local-data")
        .join("agent-control");

    for path in [
        AGENT_CONTROL_LOCAL_DATA_DIR,
        AGENT_CONTROL_DATA_DIR,
        AGENT_CONTROL_LOG_DIR,
        ac_config_directory.to_string_lossy().to_string().as_str(),
    ] {
        std::fs::create_dir_all(path).map_err(|err| {
            CliError::Command(format!("Error creating directory '{path}': {err}"))
        })?;
    }
    Ok((ac_binary_directory, ac_config_directory))
}

fn copy_agent_control_binary(ac_binary_dir: &Path) -> Result<(), CliError> {
    let current_dir = env::current_dir()
        .map_err(|err| CliError::Command(format!("Error obtaining source directory: {err}")))?;

    let binary_path = current_dir.join(EXECUTABLE_NAME);
    if !binary_path.is_file() {
        return Err(CliError::Command(format!(
            "The source binary in {} does not exist",
            binary_path.to_string_lossy(),
        )));
    }
    let destination_path = ac_binary_dir.join(EXECUTABLE_NAME);
    std::fs::copy(&binary_path, &destination_path).map_err(|err| {
        CliError::Command(format!(
            "Error copying binary from {} to {}: {}",
            binary_path.to_string_lossy(),
            destination_path.to_string_lossy(),
            err
        ))
    })?;
    Ok(())
}

// TODO: make config-generation configurable
fn generate_config(output_dir: &Path) -> Result<(), CliError> {
    let output_path = output_dir.join("config.yaml").to_string_lossy().to_string(); // TODO: use constant
    let args = vec![
        "--fleet-disabled",
        "--output-path",
        &output_path,
        "--region",
        "us",
        "--agent-set",
        "no-agents",
    ];
    let cmd = config_gen::Args::command().no_binary_name(true);
    let args = cmd
        .try_get_matches_from(args)
        .and_then(|matches| config_gen::Args::from_arg_matches(&matches))
        .map_err(|err| CliError::Command(format!("Error generating configuration: {err}")))?;
    config_gen::generate_config(args)?;
    Ok(())
}

fn install_service(binary_path: PathBuf) -> Result<(), CliError> {
    let manager_access = ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE;
    let service_manager =
        ServiceManager::local_computer(None::<&str>, manager_access).map_err(|err| {
            CliError::Command(format!("Could not interact Windows Service manager: {err}"))
        })?;

    info!(
        service_name = SERVICE_NAME,
        binary_path = binary_path.to_string_lossy().to_string(),
        "Creating and starting service..."
    );
    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart, // Start automatically on system startup
        error_control: ServiceErrorControl::Normal,
        executable_path: binary_path,
        launch_arguments: vec![],
        dependencies: vec![],
        account_name: None, // Run as System
        account_password: None,
    };
    let service = service_manager
        .create_service(
            &service_info,
            ServiceAccess::CHANGE_CONFIG | ServiceAccess::START,
        )
        .map_err(|err| {
            CliError::Command(format!(
                "Could not create New Relic Agent Control service: {err}"
            ))
        })?;
    service
        .set_description(SERVICE_DISPLAY_NAME)
        .map_err(|err| {
            CliError::Command(format!(
                "Could not set New Relic Agent Control description: {err}"
            ))
        })?;
    service
        .start(&[std::ffi::OsStr::new("Start service")])
        .map_err(|err| {
            let details = err
                .source()
                .map(|s| format!(". Caused by: {s}"))
                .unwrap_or_default();
            CliError::Command(format!(
                "Could not start {SERVICE_NAME} service: {err}{details}"
            ))
        })
}

fn stop_and_delete_previous_service_if_exists() -> Result<(), CliError> {
    let manager_access = ServiceManagerAccess::CONNECT;
    let service_manager =
        ServiceManager::local_computer(None::<&str>, manager_access).map_err(|err| {
            CliError::Command(format!("Could not interact Windows Service manager: {err}"))
        })?;
    let service_access = ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE;
    let service = match service_manager.open_service(SERVICE_NAME, service_access) {
        Ok(service) => service,
        Err(windows_service::Error::Winapi(err))
            if err.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) =>
        {
            return Ok(());
        }
        Err(err) => {
            return Err(CliError::Command(format!(
                "Could not read the '{SERVICE_NAME}' Windows service: {err}"
            )));
        }
    };
    let status = service.query_status().map_err(|err| {
        CliError::Command(format!("Error checking the '{SERVICE_NAME}' status: {err}"))
    })?;
    if status.current_state != ServiceState::Stopped {
        info!("Stopping existing '{SERVICE_NAME}' service");
        service.stop().map_err(|err| {
            CliError::Command(format!("Error stopping '{SERVICE_NAME}' service: {err}"))
        })?;
    }
    info!("Deleting existing '{SERVICE_NAME}' service");
    service.delete().map_err(|err| {
        CliError::Command(format!("Error deleting '{SERVICE_NAME}' service: {err}"))
    })?;

    drop(service);

    wait_for_service_deletion(service_manager)?;

    Ok(())
}

fn wait_for_service_deletion(service_manager: ServiceManager) -> Result<(), CliError> {
    info!("Waiting for service '{SERVICE_NAME}' deletion");
    const MAX_RETRIES: i32 = 30;
    const WAIT_TIME: Duration = Duration::from_secs(1);
    for _ in 0..MAX_RETRIES {
        if let Err(windows_service::Error::Winapi(err)) =
            service_manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS)
            && err.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32)
        {
            info!("Service '{SERVICE_NAME}' deleted successfully");
            return Ok(());
        }
        sleep(WAIT_TIME);
    }
    Err(CliError::Command(format!(
        "Timeout deleting previous service '{SERVICE_NAME}'"
    )))
}
