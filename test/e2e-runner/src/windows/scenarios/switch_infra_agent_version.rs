use crate::common::config::update_config;
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{Args, RecipeData};
use crate::windows::install::{SERVICE_NAME, tear_down_test};
use crate::windows::scenarios::DEFAULT_STATUS_PORT;
use crate::windows::service::STATUS_RUNNING;
use crate::{
    common::{config, nrql},
    windows::{self, install::install_agent_control_from_recipe},
};
use std::thread;
use std::time::Duration;
use tracing::info;

/// Windows-specific fleet for ac-e2e-onhost-win-1
const _FLEET_ID: &str =
    "NjQyNTg2NXxOR0VQfEZMRUVUfDAxOWM4YWE5LWM3YTgtN2I0ZS04NGE3LWU1YmE3NDRlNTM4Mw";

/// Windows path for environment variables file
const ENV_VARS_FILE: &str =
    r"C:\Program Files\New Relic\newrelic-agent-control\environment_variables.yaml";

pub fn switch_infra_agent_version(args: Args) {
    // We assume the below two are valid version strings, but we do not actually parse them
    let update_from_infra_agent_version = args
        .update_from_infra_agent_version
        .clone()
        .expect("--update-from-infra-agent-version is required for this scenario");

    let update_to_infra_agent_version = args
        .infra_agent_version
        .clone()
        .expect("--infra-agent-version is required for this scenario");

    assert!(
        update_from_infra_agent_version != update_to_infra_agent_version,
        "--update-from-infra-agent-version and --infra-agent-version must be different versions for this test to be meaningful. Provided version: {update_from_infra_agent_version}"
    );

    // Setup recipe data with fleet configuration
    let recipe_data = RecipeData {
        args,
        monitoring_source: "".to_string(), // Windows uses empty monitoring_source
        fleet_enabled: false.to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    // Install Agent Control with fleet disabled
    install_agent_control_from_recipe(&recipe_data);

    // Generate unique test ID with timestamp
    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    // Set up TEST_ID environment variable
    info!("Setting up `TEST_ID` environment variable");
    config::append_to_config_file(ENV_VARS_FILE, format!("TEST_ID: {test_id}").as_str());

    info!("Adding infra-agent to AC config");
    update_config(
        windows::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nr-infra:
    agent_type: newrelic/com.newrelic.infrastructure:0.1.0
"#
        ),
    );

    // Setup infra-agent config
    info!("Setup infra-agent config");
    config::write_agent_local_config(
        &windows::local_config_path("nr-infra"),
        format!(
            r#"
config_agent:
  enable_process_metrics: true
  status_server_enabled: true
  status_server_port: 18003
  license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
  custom_attributes:
    test_id: '{{{{TEST_ID}}}}'
version: {update_from_infra_agent_version}
"#
        ),
    );

    // Restart service and wait for it to be running
    windows::service::restart_service(SERVICE_NAME, STATUS_RUNNING);

    info!("Waiting 10 seconds for service to start");
    thread::sleep(Duration::from_secs(10));

    windows::service::check_service_status(SERVICE_NAME, STATUS_RUNNING)
        .expect("service should be running");

    info!("Verifying service health");
    let status_endpoint = format!("http://localhost:{DEFAULT_STATUS_PORT}/status");
    let status = retry_panic(30, Duration::from_secs(2), "health check", || {
        windows::health::check_health(&status_endpoint)
    });

    info!("Agent Control is healthy");
    let status_json = serde_json::to_string_pretty(&status)
        .unwrap_or_else(|err| panic!("Failed to serialize status to JSON: {err}"));
    info!(response = status_json, "Agent Control is healthy");

    // Validate infra agent is reporting with local config
    info!("Check infra agent is reporting");
    let nrql_query = format!(
        r#"SELECT * FROM SystemSample WHERE `test_id` = '{test_id}' AND `agentVersion` = '{update_from_infra_agent_version}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(5), "nrql assertion", || {
        nrql::check_query_results(&recipe_data.args, &nrql_query, |r| !r.is_empty())
    });

    // Now change the version for the infra-agent installation, restart AC and check everything all
    // over again.

    // Setup infra-agent config with different version
    info!("Replace infra-agent version");
    config::modify_agents_config(
        windows::local_config_path("nr-infra"),
        update_from_infra_agent_version.to_string().as_str(),
        update_to_infra_agent_version.to_string().as_str(),
    );

    // Restart service and wait for it to be running
    windows::service::restart_service(SERVICE_NAME, STATUS_RUNNING);

    info!("Waiting 10 seconds for service to start");
    thread::sleep(Duration::from_secs(10));

    windows::service::check_service_status(SERVICE_NAME, STATUS_RUNNING)
        .expect("service should be running");

    info!("Verifying service health");
    let status_endpoint = format!("http://localhost:{DEFAULT_STATUS_PORT}/status");
    let status = retry_panic(30, Duration::from_secs(2), "health check", || {
        windows::health::check_health(&status_endpoint)
    });

    info!("Agent Control is healthy");
    let status_json = serde_json::to_string_pretty(&status)
        .unwrap_or_else(|err| panic!("Failed to serialize status to JSON: {err}"));
    info!(response = status_json, "Agent Control is healthy");

    // Validate remote configuration has been applied
    info!("Check that remote configuration has been applied and agent update occurred");
    let nrql_query = format!(
        r#"SELECT * FROM SystemSample WHERE `test_id` = '{test_id}' AND `agentVersion` = '{update_to_infra_agent_version}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(5), "nrql assertion", || {
        nrql::check_query_results(&recipe_data.args, &nrql_query, |r| !r.is_empty())
    });
}
