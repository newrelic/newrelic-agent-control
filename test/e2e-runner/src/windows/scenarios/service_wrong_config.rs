use crate::common::config::modify_agents_config;
use crate::common::on_drop::CleanUp;
use crate::common::test::retry;
use crate::common::{Args, RecipeData};
use crate::windows::install::{SERVICE_NAME, install_agent_control_from_recipe, tear_down_test};
use crate::windows::scenarios::DEFAULT_STATUS_PORT;
use crate::windows::service::{STATUS_RUNNING, STATUS_STOPPED};
use crate::windows::{self};
use std::thread;
use std::time::Duration;
use tracing::info;

/// Runs a Windows E2E installation test modifying the config for a wrong one and restarting the service
/// to ensure it stops, then sets again a correct config and ensures the service runs correctly.
pub fn test_service_restart_depending_on_config_correctness(args: Args) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    // Replace the valid empty map with an unclosed one to break YAML parsing
    modify_agents_config(windows::DEFAULT_CONFIG_PATH, "agents: {}", "agents: {");

    // Expect restart to fail
    windows::service::restart_service(SERVICE_NAME, STATUS_STOPPED);
    info!("Waiting 10 seconds for service to start");
    thread::sleep(Duration::from_secs(10));

    windows::service::check_service_status(SERVICE_NAME, STATUS_STOPPED)
        .expect("service shouldn't be running");

    // Adding a correct configuration
    // Replace the unclosed one to break YAML parsing back to a valid empty map
    modify_agents_config(windows::DEFAULT_CONFIG_PATH, "agents: {", "agents: {}");

    // Expect restart to succeed
    windows::service::restart_service(SERVICE_NAME, STATUS_RUNNING);
    info!("Waiting 10 seconds for service to start");
    thread::sleep(Duration::from_secs(10));

    windows::service::check_service_status(SERVICE_NAME, STATUS_RUNNING)
        .expect("service shouldn't be running");

    info!("Verifying service health");
    let status_endpoint = format!("http://localhost:{DEFAULT_STATUS_PORT}/status");
    let status = retry(30, Duration::from_secs(2), "health check", || {
        windows::health::check_health(&status_endpoint)
    })
    .unwrap_or_else(|err| panic!("Health check failed: {err}"));

    info!("Agent Control is healthy");
    let status_json = serde_json::to_string_pretty(&status)
        .unwrap_or_else(|err| panic!("Failed to serialize status to JSON: {err}"));
    info!(response = status_json, "Agent Control is healthy");

    info!("Test completed successfully");
}
