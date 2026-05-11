use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::{
    common::nrql,
    linux::{
        self,
        install::{install_agent_control_from_recipe, tear_down_test},
    },
};
use std::time::Duration;
use tracing::info;

pub fn test_installation_with_preload_agent(args: InstallationArgs) {
    let preload_version = args
        .preload_version
        .clone()
        .expect("--preload-agent-version is required for this scenario");

    let staging = matches!(args.nr_region.to_lowercase().as_str(), "staging");

    let recipe_data = RecipeData {
        args,
        monitoring_source: "preload-agent".to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-preload-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    let preload_agent_id = "nr-preload";

    info!("Setup Agent Control config");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nr-preload:
    agent_type: "newrelic/com.newrelic.preload:0.1.0"
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

    write_agent_local_config(
        &linux::local_config_path(preload_agent_id),
        // Correct Config?
        format! {r#"
fleet_id: alphanumeric_id # needed anymore?
apm_language: java
agent_version: 8.13.0
application_names:
  - my-app
  - functions
  - lib
  - bin
new_relic_license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
staging: {staging}
version: {preload_version}"#},
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    let nrql_query = format!(r#"SELECT * FROM SystemSample WHERE `host.id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Checking results of NRQL to check logs");
    let retries = 30;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    info!("Test completed successfully");
}
