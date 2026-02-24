use crate::common::config::update_config;
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{Args, RecipeData};
use crate::linux::install::tear_down_test;
use crate::{
    common::{config, nrql},
    linux::{self, install::install_agent_control_from_recipe},
};
use std::time::Duration;
use tracing::info;

/// Linux path for environment variables file
const ENV_VARS_FILE: &str = "/etc/newrelic-agent-control/environment_variables.yaml";

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

    let recipe_data = RecipeData {
        args,
        monitoring_source: "infra-agent".to_string(),
        fleet_enabled: false.to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-infra-agent_switch-version_linux_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    info!("Setting up `TEST_ID` environment variable");
    config::append_to_config_file(ENV_VARS_FILE, format!("TEST_ID: {test_id}").as_str());

    info!("Adding infra-agent to AC config");
    let debug_log_config = config::ac_debug_logging_config(linux::DEFAULT_LOG_PATH);
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
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

    info!("Setup infra-agent config");
    config::write_agent_local_config(
        &linux::local_config_path("nr-infra"),
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

    linux::service::restart_service(linux::SERVICE_NAME);

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

    info!("Replace infra-agent version");
    config::modify_agents_config(
        linux::local_config_path("nr-infra"),
        update_from_infra_agent_version.to_string().as_str(),
        update_to_infra_agent_version.to_string().as_str(),
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    info!("Check that agent update occurred");
    let nrql_query = format!(
        r#"SELECT * FROM SystemSample WHERE `test_id` = '{test_id}' AND `agentVersion` = '{update_to_infra_agent_version}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 120; // This might take a while, so duplicating retries
    retry_panic(retries, Duration::from_secs(5), "nrql assertion", || {
        nrql::check_query_results(&recipe_data.args, &nrql_query, |r| !r.is_empty())
    });
}
