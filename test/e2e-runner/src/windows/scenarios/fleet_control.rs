use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config};
use crate::common::fleet_control_api;
use crate::common::on_drop::CleanUp;
use crate::common::{InstallationArgs, RecipeData};
use crate::windows;
use crate::windows::install::{install_agent_control_from_recipe, tear_down_test};
use crate::windows::service::STATUS_RUNNING;
use std::time::Duration;
use tracing::info;

pub fn test_fleet_control(args: InstallationArgs) {
    let FleetControlArgs {fleet_id, fleet_control_token, fleet_type, test_suite} = args.fleet_control.as_ref().expect("Fleet Control configs (--fleet-id, --fleet-control-token, --fleet-type, --test-suite) must be added for running the fleet-control scenario");

    assert_eq!(
        args.nr_region.to_lowercase().as_str(),
        "staging",
        "This test can only run on staging environment"
    );

    info!("Starting Fleet Control E2E test on Windows");
    info!("Using Fleet ID: {fleet_id}");

    let recipe_data = RecipeData {
        args: args.clone(),
        fleet_enabled: true,
        fleet_id: fleet_id.clone(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    info!("Installing Agent Control with Fleet Control configuration");
    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-fleet-control_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    info!("Configuring Agent Control for Fleet Control");
    update_config(
        windows::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  infra:
    agent_type: newrelic/com.newrelic.infrastructure:0.1.0
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

    info!("Restarting Agent Control service");
    windows::service::restart_service(windows::install::SERVICE_NAME, STATUS_RUNNING);

    // Wait a bit for Agent Control to start and connect to Fleet Control
    info!("Waiting for Agent Control to connect to Fleet Control...");
    std::thread::sleep(Duration::from_secs(30));

    // Trigger Fleet Control tests and wait for completion
    let test_response = fleet_control_api::trigger_and_wait_for_fleet_control_tests(
        fleet_id,
        fleet_control_token,
        fleet_type,
        test_suite,
    );

    // Write test report to JSON file
    fleet_control_api::write_test_report(&test_response);

    // Check if tests failed and exit with error if so
    if test_response.is_failed() {
        panic!(
            "❌ Tests failed: {} failed, {} inconclusive",
            test_response.failed_count, test_response.inconclusive_count
        );
    }
}
