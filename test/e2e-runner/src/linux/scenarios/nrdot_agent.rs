use std::time::Duration;

use tracing::info;

use crate::{
    linux::{
        self,
        install::{Args, RecipeData, install_agent_control_from_recipe},
    },
    tools::{config, logs::ShowLogsOnDrop, nrql, test::retry},
};

pub fn test_nrdot_agent(args: Args) {
    let recipe_data = RecipeData {
        args,
        monitoring_source: "otel".to_string(),
        recipe_list: "agent-control".to_string(),
        ..Default::default()
    };
    install_agent_control_from_recipe(&recipe_data);
    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    info!("Setup Agent Control config with nr-dot");
    let debug_log_config = config::debug_logging_config(linux::DEFAULT_LOG_PATH);
    let config = format!(
        r#"
host_id: {test_id}
{debug_log_config}
"#
    );
    config::update_config(linux::DEFAULT_CONFIG_PATH, &config);

    linux::service::restart_service(linux::SERVICE_NAME);
    let _show_logs = ShowLogsOnDrop::from(linux::DEFAULT_LOG_PATH);

    let nrql_query = format!(
        r#"SELECT `system.memory.utilization` FROM Metric WHERE `host.id` = '{test_id}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 120;
    retry(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    })
    .unwrap_or_else(|err| {
        panic!("query '{nrql_query}' failed after {retries} retries: {err}");
    });
}
