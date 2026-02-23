use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{Args, RecipeData};
use crate::windows::install::{SERVICE_NAME, tear_down_test};
use crate::windows::service::STATUS_RUNNING;
use crate::{
    common::{config, nrql},
    windows::{self, install::install_agent_control_from_recipe},
};
use std::time::Duration;
use tracing::info;

/// Windows-specific fleet for ac-e2e-onhost-win-1
const FLEET_ID: &str = "NjQyNTg2NXxOR0VQfEZMRUVUfDAxOWM4YWE5LWM3YTgtN2I0ZS04NGE3LWU1YmE3NDRlNTM4Mw";

// As of writing this test the latest version is 1.72.4 so an update should be triggered.
const STARTING_NEWRELIC_INFRA_VERSION: &str = "1.72.1";

/// Windows path for environment variables file
/// TODO: Verify this file exists after installation and supports environment variable interpolation
const ENV_VARS_FILE: &str =
    r"C:\Program Files\New Relic\newrelic-agent-control\environment_variables.yaml";

pub fn test_remote_config_with_version_is_applied(args: Args) {
    // Setup recipe data with fleet configuration
    let recipe_data = RecipeData {
        args,
        monitoring_source: "".to_string(), // Windows uses empty monitoring_source
        fleet_enabled: "true".to_string(),
        fleet_id: FLEET_ID.to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    // Install Agent Control with fleet enabled
    install_agent_control_from_recipe(&recipe_data);

    // Generate unique test ID with timestamp
    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    // Set up TEST_ID environment variable
    // TODO: Verify environment_variables.yaml is supported on Windows
    // If not, use host.id pattern like installation_infra_agent.rs instead
    info!("Setting up `TEST_ID` environment variable");
    config::append_to_config_file(ENV_VARS_FILE, format!("TEST_ID: {test_id}").as_str());

    // Setup Agent Control config for debug logging
    info!("Setup Agent Control config for debug logging");
    config::update_config_for_debug_logging(
        windows::DEFAULT_AC_CONFIG_PATH,
        windows::DEFAULT_LOG_PATH,
    );

    // Setup infra-agent config with local custom attribute
    info!("Setup infra-agent config");
    config::write_agent_local_config(
        &windows::local_config_path("nr-infra"),
        format!(
            r#"
config_agent:
  enable_process_metrics: true
  status_server_enabled: true
  status_server_port: 18003
  license_key: {{{{NEW_RELIC_LICENSE_KEY}}}}
  custom_attributes:
    config_origin: local
    test_id: {{{{TEST_ID}}}}
version: {STARTING_NEWRELIC_INFRA_VERSION}
"#
        ),
    );

    // Restart service and wait for it to be running
    windows::service::restart_service(SERVICE_NAME, STATUS_RUNNING);

    // Validate infra agent is reporting with local config
    info!("Check infra agent is reporting");
    let nrql_query = format!(r#"SELECT * FROM SystemSample WHERE `test_id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(5), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    // Validate remote configuration has been applied
    info!("Check that remote configuration has been applied");
    let nrql_query = format!(
        r#"SELECT * FROM SystemSample WHERE `test_id` = '{test_id}' AND `config_origin` = 'remote' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });
}
