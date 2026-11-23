use crate::common::agent_metadata::{AgentKind, AgentMetadata};
use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config};
use crate::common::health::wait_until_healthy;
use crate::common::ohi::{
    EMBEDDED_LINUX_OHI_BINARIES, EMBEDDED_LINUX_OHI_CONFIGS, SHARED_LINUX_FILESYSTEM_DIR,
    check_ohi_shared_filesystem,
};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux::scenarios::DEFAULT_STATUS_PORT;
use crate::{
    common::{config, nrql},
    linux::{
        self,
        install::{install_agent_control_from_recipe, tear_down_test},
    },
};
use std::time::Duration;
use tracing::info;

const ENV_VARS_FILE: &str = "/etc/newrelic-agent-control/environment_variables.yaml";

/// Agent type compatible with `UPDATE_FROM_INFRA_AGENT_VERSION`; newer agent types require
/// artifacts that this package version does not ship.
const UPDATE_FROM_INFRA_AGENT_TYPE: &str = "newrelic/com.newrelic.infrastructure:0.1.0";
const UPDATE_FROM_INFRA_AGENT_VERSION: &str = "1.71.4";

pub fn switch_infra_agent_version(args: InstallationArgs) {
    let AgentMetadata {
        sub_agent_id: infra_sub_agent_id,
        agent_type_id: infra_agent_type,
        package_version: update_to_infra_agent_version,
    } = args.agent(AgentKind::InfraAgent);
    assert!(
        UPDATE_FROM_INFRA_AGENT_VERSION != update_to_infra_agent_version,
        "the resolved infra agent version must be different from the built-in update-from version ({UPDATE_FROM_INFRA_AGENT_VERSION}) for this test to be meaningful."
    );

    let recipe_data = RecipeData {
        args,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-infra-agent_switch-version_linux_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    info!("Setting up `TEST_ID` environment variable");
    config::append_to_config_file(ENV_VARS_FILE, format!("TEST_ID: {test_id}").as_str());

    info!("Adding infra-agent to AC config");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  {infra_sub_agent_id}:
    agent_type: {UPDATE_FROM_INFRA_AGENT_TYPE}
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

    info!("Setup infra-agent config");
    config::write_agent_local_config(
        &linux::local_config_path(infra_sub_agent_id),
        format!(
            r#"
config_agent:
  license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
  custom_attributes:
    test_id: '{{{{TEST_ID}}}}'
version: {UPDATE_FROM_INFRA_AGENT_VERSION}
"#
        ),
    );

    linux::service::restart_service(linux::SERVICE_NAME);
    wait_until_healthy(DEFAULT_STATUS_PORT);

    info!("Check infra agent is reporting");
    let nrql_query = format!(
        r#"SELECT * FROM SystemSample WHERE `test_id` = '{test_id}' AND `agentVersion` = '{UPDATE_FROM_INFRA_AGENT_VERSION}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(5), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    // Now change the agent type and version for the infra-agent installation, restart AC and check
    // everything all over again.

    info!("Replace infra-agent agent type");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
agents:
  {infra_sub_agent_id}:
    agent_type: {infra_agent_type}
"#
        ),
    );

    info!("Replace infra-agent version");
    config::modify_agents_config(
        linux::local_config_path(infra_sub_agent_id),
        UPDATE_FROM_INFRA_AGENT_VERSION,
        update_to_infra_agent_version.as_str(),
    );

    linux::service::restart_service(linux::SERVICE_NAME);
    wait_until_healthy(DEFAULT_STATUS_PORT);

    info!("Check that agent update occurred");
    let nrql_query = format!(
        r#"SELECT * FROM SystemSample WHERE `test_id` = '{test_id}' AND `agentVersion` = '{update_to_infra_agent_version}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 120; // This might take a while, so duplicating retries
    retry_panic(retries, Duration::from_secs(5), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    info!(
        "Checking OHI binaries and configs of the updated agent type are in the shared filesystem"
    );
    retry_panic(
        30,
        Duration::from_secs(2),
        "shared filesystem OHI binaries and configs",
        || {
            check_ohi_shared_filesystem(
                SHARED_LINUX_FILESYSTEM_DIR,
                EMBEDDED_LINUX_OHI_BINARIES.as_slice(),
                EMBEDDED_LINUX_OHI_CONFIGS.as_slice(),
            )
        },
    );
}
