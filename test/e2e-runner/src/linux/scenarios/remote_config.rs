use crate::common::nrql::Region;
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux::install::tear_down_test;
use crate::{
    common::{config, nrql},
    linux::{self, install::install_agent_control_from_recipe},
};
use std::time::Duration;
use tracing::info;

/// ac-e2e-onhost-2-9-9 fleet on prod canaries account
const FLEET_ID: &str = "NjQyNTg2NXxOR0VQfEZMRUVUfDAxOTkyOGQyLTg3OTAtNzJlNC05ODgwLTJhYzE0NTRlZDUyZg";
/// ac-e2e-onhost-2-9-9 fleet on staging canaries account
const FLEET_ID_STAGING: &str =
    "MTIyMTMwNjh8TkdFUHxGTEVFVHwwMTlkMDY2Yy02YzRmLTdkOTgtYWNlMC1hNDZmY2NjNGU3ODM";

const ENV_VARS_FILE: &str = "/etc/newrelic-agent-control/environment_variables.yaml";

pub fn test_remote_config_is_applied(args: InstallationArgs) {
    let fleet_id = match args.nr_region {
        Region::Staging => FLEET_ID_STAGING.to_string(),
        _ => FLEET_ID.to_string(),
    };

    let recipe_data = RecipeData {
        args,
        fleet_enabled: true,
        fleet_id,
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    info!("Setting up `TEST_ID` environment variable");
    config::append_to_config_file(ENV_VARS_FILE, format!("TEST_ID: {test_id}").as_str());

    info!("Setup Agent Control config for debug logging");
    config::update_config_for_debug_logging(linux::DEFAULT_AC_CONFIG_PATH);

    linux::service::restart_service(linux::SERVICE_NAME);

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
