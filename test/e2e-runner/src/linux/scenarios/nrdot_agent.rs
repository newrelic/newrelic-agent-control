use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{InstallationArgs, RecipeData};
use crate::linux::install::tear_down_test;
use crate::{
    common::nrql,
    linux::{self, install::install_agent_control_from_recipe},
};
use std::time::Duration;
use tracing::info;

pub fn test_nrdot_agent(args: InstallationArgs) {
    let recipe_data = RecipeData {
        args,
        monitoring_source: "network".to_string(),
        recipe_list: "agent-control".to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);
    let test_id = format!(
        "onhost-e2e-network-flow_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    info!("Setup Agent Control config with network flow monitoring agent");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nrdot:
    agent_type: newrelic/com.newrelic.opentelemetry.collector:0.1.0
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

    write_agent_local_config(
        &linux::local_config_path("nrdot"),
        format!(
            r#"
nr_account_id: "{}"
container_name: "ktranslate-flow-e2e"
flow_port: 9995
"#,
            recipe_data.args.nr_account_id
        ),
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    let nrql_query = format!(
        r#"SELECT count(*) FROM KFlow WHERE `host.id` = '{test_id}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });
}
