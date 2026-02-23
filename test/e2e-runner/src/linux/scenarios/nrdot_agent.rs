use crate::common::config::nrdot_config;
use crate::common::config::{ac_debug_logging_config, update_config, write_agent_local_config};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{Args, RecipeData};
use crate::linux::install::tear_down_test;
use crate::{
    common::nrql,
    linux::{self, install::install_agent_control_from_recipe},
};
use std::time::Duration;
use tracing::info;

pub fn test_nrdot_agent(args: Args) {
    let nrdot_version = args
        .nrdot_version
        .clone()
        .expect("--nrdot-version is required for this scenario");

    let recipe_data = RecipeData {
        args,
        monitoring_source: "otel".to_string(),
        recipe_list: "agent-control".to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);
    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    info!("Setup Agent Control config with nr-dot");
    let debug_log_config = ac_debug_logging_config(linux::DEFAULT_LOG_PATH);
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nrdot:
    agent_type: newrelic/com.newrelic.opentelemetry.collector:0.1.0
{debug_log_config}
"#
        ),
    );

    write_agent_local_config(
        &linux::local_config_path("nrdot"),
        nrdot_config(&nrdot_version),
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    let nrql_query = format!(
        r#"SELECT `system.memory.utilization` FROM Metric WHERE `host.id` = '{test_id}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });
}
