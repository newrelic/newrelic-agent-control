use std::thread;
use std::time::Duration;

use tracing::info;

use crate::tools::config;
use crate::tools::logs::ShowLogsOnDrop;
use crate::tools::test::retry;
use crate::windows;
use crate::windows::install::{Args, RecipeData, install_agent_control_from_recipe};

const DEFAULT_STATUS_PORT: u16 = 51200;
const SERVICE_NAME: &str = "newrelic-agent-control";

/// Runs a complete Windows E2E installation test.
pub fn test_installation(args: Args) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };
    install_agent_control_from_recipe(&recipe_data);

    info!("Waiting 10 seconds for service to start");
    thread::sleep(Duration::from_secs(10));

    windows::service::check_service_running(SERVICE_NAME).expect("service should be running");

    config::update_config_for_debug_logging(
        windows::DEFAULT_CONFIG_PATH,
        windows::DEFAULT_LOG_PATH,
    );

    windows::service::restart_service(SERVICE_NAME);
    info!("Waiting 10 seconds for service to start");
    thread::sleep(Duration::from_secs(10));

    let _show_logs = ShowLogsOnDrop::from(windows::DEFAULT_CONFIG_PATH);

    info!("Verifying service health");
    let status_endpoint = format!("http://localhost:{DEFAULT_STATUS_PORT}/status");
    let status = retry(30, Duration::from_secs(2), "health check", || {
        windows::health::check_health(&status_endpoint)
    })
    .unwrap(); // TODO
    info!("Agent Control is healthy");
    let status_json = serde_json::to_string_pretty(&status).unwrap(); // TODO
    info!(response = status_json, "Agent Control is healthy");

    windows::cleanup::cleanup(SERVICE_NAME);
}
