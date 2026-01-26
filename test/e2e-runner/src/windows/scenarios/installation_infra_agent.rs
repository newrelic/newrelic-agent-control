use crate::common::config::{ac_debug_logging_config, update_config, write_agent_local_config};
use crate::common::logs::ShowLogsOnDrop;
use crate::common::test::retry;
use crate::common::{Args, RecipeData, nrql};
use crate::windows;
use crate::windows::install::install_agent_control_from_recipe;
use std::thread;
use std::time::Duration;
use tracing::info;

const DEFAULT_STATUS_PORT: u16 = 51200;
const SERVICE_NAME: &str = "newrelic-agent-control";

/// Runs a complete Windows E2E installation test.
pub fn test_infra_agent(args: Args) {
    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };
    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    let debug_log_config = ac_debug_logging_config(windows::DEFAULT_LOG_PATH);

    update_config(
        windows::DEFAULT_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nr-infra:
    agent_type: newrelic/com.newrelic.infrastructure:0.1.0
{debug_log_config}
"#
        ),
    );

    // TODO we should get the version dynamically from the recipe itself
    write_agent_local_config(
        windows::DEFAULT_NR_INFRA_PATH,
        r#"
config_agent:
  license_key: '{{NEW_RELIC_LICENSE_KEY}}'
version: v1.71.4
"#
        .to_string(),
    );

    windows::service::restart_service(SERVICE_NAME);
    info!("Waiting 10 seconds for service to start");
    thread::sleep(Duration::from_secs(10));

    // At the end of the test, we print the logs.
    let _show_logs = ShowLogsOnDrop::from(windows::DEFAULT_LOG_PATH);

    info!("Waiting 10 seconds for service to start");
    thread::sleep(Duration::from_secs(10));

    windows::service::check_service_running(SERVICE_NAME).expect("service should be running");

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

    let nrql_query = format!(r#"SELECT * FROM SystemSample WHERE `host.id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    })
    .unwrap_or_else(|err| {
        panic!("query '{nrql_query}' failed after {retries} retries: {err}");
    });

    info!("Test completed successfully");
}
