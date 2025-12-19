use std::thread;
use std::time::Duration;

use crate::tools::config;
use crate::tools::logs::show_logs;
use crate::tools::test::TestResult;
use crate::windows;
use tracing::info;

const DEFAULT_STATUS_PORT: u16 = 51200;
const SERVICE_NAME: &str = "newrelic-agent-control";

#[derive(Debug, clap::Parser)]
pub struct Args {
    /// Path to the Windows zip package file
    #[arg(short, long)]
    zip_package: String,
}

/// Runs a complete Windows E2E installation test.
pub fn test_installation(args: Args) -> TestResult<()> {
    let zip_package = &args.zip_package;
    info!("Installing agent-control...");
    windows::install::install_agent_control(zip_package, true)?;

    info!("Waiting 10 seconds for service to start...");
    thread::sleep(Duration::from_secs(10));

    info!("Checking Windows service status");
    windows::service::check_service_running(SERVICE_NAME)?;

    info!("Updating configuration for debug logging");
    config::update_config_for_debug_logging(
        windows::DEFAULT_CONFIG_PATH,
        windows::DEFAULT_LOG_PATH,
    )?;

    info!("Restarting service to apply configuration");
    windows::service::restart_service(SERVICE_NAME)?;
    thread::sleep(Duration::from_secs(10));

    info!("Verifying service health");
    let status_endpoint = format!("http://localhost:{}/status", DEFAULT_STATUS_PORT);
    let status =
        windows::health::check_health_with_retry(&status_endpoint, 30, Duration::from_secs(2))?;
    info!("Agent Control is healthy");
    let status_json = serde_json::to_string_pretty(&status)?;
    info!("Status response: {}", status_json);

    show_logs(windows::DEFAULT_LOG_PATH)?;
    windows::cleanup::cleanup(SERVICE_NAME)?;
    Ok(())
}
