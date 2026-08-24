use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::nrql;
use crate::common::ohi::{
    EMBEDDED_WINDOWS_OHI_BINARIES, SHARED_WINDOWS_FILESYSTEM_DIR, check_ohi_shared_filesystem,
};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::windows;
use crate::windows::install::{SERVICE_NAME, install_agent_control_from_recipe, tear_down_test};
use crate::windows::service::{STATUS_RUNNING, restart_service};
use std::time::Duration;
use tracing::info;

pub fn test_nri_flex(args: InstallationArgs) {
    let infra_agent_version = args
        .infra_agent_version
        .clone()
        .expect("--infra-agent-version is required for this scenario");

    let flex_version = args
        .flex_version
        .clone()
        .expect("--flex-version is required for this scenario");

    let test_id = format!(
        "onhost-e2e-nri-flex_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f"),
    );

    let recipe_data = RecipeData {
        args: args.clone(),
        ..Default::default()
    };
    let _clean_up = CleanUp::new(tear_down_test);
    install_agent_control_from_recipe(&recipe_data);

    update_config(
        windows::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
  nr-flex:
    agent_type: "newrelic/com.newrelic.infrastructure.nri_flex:0.1.0"
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

    write_agent_local_config(
        &windows::local_config_path("nr-infra"),
        format!(
            r#"
config_agent:
  license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
  log:
    level: debug
version: {infra_agent_version}
"#
        ),
    );

    // Exercises the nri-flex `files` variable: two independent files are rendered by AC under
    // ${{nr-path:agent_dir}}\files\ and each is wired into its own `commands` API below. This
    // proves both that multiple map entries land on disk and that each is independently usable
    // from the integration config.
    write_agent_local_config(
        &windows::local_config_path("nr-flex"),
        format!(
            r#"
files:
  metric_one.ps1: |
    Write-Output '{{"metric_one_value": 111}}'
  metric_two.ps1: |
    Write-Output '{{"metric_two_value": 222}}'
config:
  integrations:
    - name: nri-flex
      interval: 15s
      config:
        name: e2e-flex
        apis:
          - event_type: FlexE2ESample
            shell: powershell
            commands:
              - run: '& "${{nr-path:agent_dir}}\files\metric_one.ps1"'
            custom_attributes:
              test.id: {test_id}
          - event_type: FlexE2ESample
            shell: powershell
            commands:
              - run: '& "${{nr-path:agent_dir}}\files\metric_two.ps1"'
            custom_attributes:
              test.id: {test_id}
version: {flex_version}
"#
        ),
    );

    restart_service(SERVICE_NAME, STATUS_RUNNING);

    let metric_one_query = format!(
        r#"SELECT * FROM FlexE2ESample WHERE `test.id` = '{test_id}' AND metric_one_value IS NOT NULL LIMIT 1"#
    );
    let metric_two_query = format!(
        r#"SELECT * FROM FlexE2ESample WHERE `test.id` = '{test_id}' AND metric_two_value IS NOT NULL LIMIT 1"#
    );
    info!(
        nrql = metric_one_query,
        "Waiting for FlexE2ESample data sourced from files.metric_one.ps1"
    );
    retry_panic(
        60,
        Duration::from_secs(10),
        "FlexE2ESample from metric_one.ps1",
        || nrql::check_query_results_are_not_empty(&recipe_data.args, &metric_one_query),
    );
    info!(
        nrql = metric_two_query,
        "Waiting for FlexE2ESample data sourced from files.metric_two.ps1"
    );
    retry_panic(
        60,
        Duration::from_secs(10),
        "FlexE2ESample from metric_two.ps1",
        || nrql::check_query_results_are_not_empty(&recipe_data.args, &metric_two_query),
    );

    info!("Verifying shared-filesystem files were populated by AC");
    let expected_binaries = [EMBEDDED_WINDOWS_OHI_BINARIES.as_slice(), &["nri-flex.exe"]].concat();
    let expected_configs = ["nri-flex.yaml"].as_slice();

    retry_panic(
        30,
        Duration::from_secs(2),
        "shared filesystem OHI binaries and configs",
        || {
            check_ohi_shared_filesystem(
                SHARED_WINDOWS_FILESYSTEM_DIR,
                &expected_binaries,
                expected_configs,
            )
        },
    );

    info!("nri-flex Windows scenario completed successfully");
}
