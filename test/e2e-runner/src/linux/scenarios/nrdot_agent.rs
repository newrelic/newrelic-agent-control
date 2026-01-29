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

pub fn test_nrdot_agent(args: Args) {
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
    config::update_config_for_debug_logging(linux::DEFAULT_CONFIG_PATH, linux::DEFAULT_LOG_PATH);
    config::update_config_for_host_id(linux::DEFAULT_CONFIG_PATH, &test_id);

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
