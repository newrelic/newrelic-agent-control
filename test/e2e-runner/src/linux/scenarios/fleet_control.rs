use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config};
use crate::common::fleet_control_api;
use crate::common::on_drop::CleanUp;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux;
use crate::linux::install::{install_agent_control_from_recipe, tear_down_test};
use std::time::Duration;
use tracing::info;

pub fn test_fleet_control(args: InstallationArgs) {
    let fleet_id = args
        .fleet_id
        .as_ref()
        .expect("--fleet-id is required for fleet-control scenario");

    let fleet_control_token = args
        .fleet_control_token
        .as_ref()
        .expect("--fleet-control-token is required for fleet-control scenario");

    let fleet_type = &args.fleet_type;

    assert_eq!(
        args.nr_region.to_lowercase().as_str(),
        "staging",
        "This test can only run on staging environment"
    );

    info!("Starting Fleet Control E2E test");
    info!("Using Fleet ID: {fleet_id}");

    let recipe_data = RecipeData {
        args: args.clone(),
        monitoring_source: "infra-agent".to_string(),
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
        linux::DEFAULT_AC_CONFIG_PATH,
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
    linux::service::restart_service(linux::SERVICE_NAME);

    // Wait a bit for Agent Control to start and connect to Fleet Control
    info!("Waiting for Agent Control to connect to Fleet Control...");
    std::thread::sleep(Duration::from_secs(30));

    // Trigger Fleet Control tests and wait for completion
    fleet_control_api::trigger_and_wait_for_fleet_control_tests(
        fleet_id,
        fleet_control_token,
        fleet_type,
    );
}
