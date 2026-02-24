use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{Args, RecipeData};
use crate::windows::install::{SERVICE_NAME, install_agent_control_from_recipe, tear_down_test};
use crate::windows::scenarios::DEFAULT_STATUS_PORT;
use crate::windows::service::STATUS_RUNNING;
use crate::{
    common::{config, nrql},
    windows::{self},
};
use std::thread;
use std::time::Duration;
use tracing::info;

/// Windows fleet for remote config testing (ac-e2e-onhost-win-1 on canaries account)
const FLEET_ID: &str = "NjQyNTg2NXxOR0VQfEZMRUVUfDAxOWM4YWE5LWM3YTgtN2I0ZS04NGE3LWU1YmE3NDRlNTM4Mw";

/// Windows path for environment variables file
const ENV_VARS_FILE: &str =
    r"C:\Program Files\New Relic\newrelic-agent-control\environment_variables.yaml";

pub fn test_remote_config_is_applied(args: Args) {
    let recipe_data = RecipeData {
        args,
        fleet_enabled: true.to_string(),
        fleet_id: FLEET_ID.to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    info!("Setting up `TEST_ID` environment variable");
    config::append_to_config_file(ENV_VARS_FILE, format!("TEST_ID: {test_id}").as_str());

    info!("Setup infra-agent config");
    config::write_agent_local_config(
        &windows::local_config_path("nr-infra"),
        r#"
config_agent:
  status_server_enabled: true
  status_server_port: 18003
  license_key: {{NEW_RELIC_LICENSE_KEY}}
  custom_attributes:
    config_origin: local
    test_id: {{TEST_ID}}
"#
        .to_string(),
    );

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

    info!("Check infra agent is reporting");
    let nrql_query = format!(r#"SELECT * FROM SystemSample WHERE `test_id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(5), "nrql assertion", || {
        nrql::check_query_results(&recipe_data.args, &nrql_query, |r| !r.is_empty())
    });

    info!("Check that remote configuration has been applied");
    let nrql_query = format!(
        r#"SELECT * FROM SystemSample WHERE `test_id` = '{test_id}' AND `config_origin` = 'remote' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results(&recipe_data.args, &nrql_query, |r| !r.is_empty())
    });
}
