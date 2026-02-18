use crate::common::config::nrdot_config;
use crate::common::config::{ac_debug_logging_config, update_config, write_agent_local_config};
use crate::common::on_drop::CleanUp;
use crate::common::test::{retry, retry_panic};
use crate::common::{Args, RecipeData, nrql};
use crate::windows::install::{SERVICE_NAME, install_agent_control_from_recipe, tear_down_test};
use crate::windows::scenarios::DEFAULT_STATUS_PORT;
use crate::windows::service::STATUS_RUNNING;
use crate::windows::{self};
use std::thread;
use std::time::Duration;
use tracing::info;

/// Runs a complete Windows E2E installation test with NRDOT
pub fn test_nrdot(args: Args) {
    let nrdot_version = args
        .nrdot_version
        .clone()
        .expect("--nrdot-version is required for this scenario");

    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-nrdot_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    let debug_log_config = ac_debug_logging_config(windows::DEFAULT_LOG_PATH);

    update_config(
        windows::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nrdot:
    agent_type: newrelic/com.newrelic.opentelemetry.collector:0.1.0
{debug_log_config}
"#
        ),
    );

    write_agent_local_config(
        &windows::local_config_path("nrdot"),
        nrdot_config(&nrdot_version),
    );

    windows::service::restart_service(SERVICE_NAME, STATUS_RUNNING);
    info!("Waiting 10 seconds for service to start");
    thread::sleep(Duration::from_secs(10));

    windows::service::check_service_status(SERVICE_NAME, STATUS_RUNNING)
        .expect("service should be running");

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

    let nrql_query = format!(
        r#"SELECT `system.memory.utilization` FROM Metric WHERE `host.id` = '{test_id}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    info!("Test completed successfully");
}
