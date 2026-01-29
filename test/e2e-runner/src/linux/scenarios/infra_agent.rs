use crate::common::test::retry_panic;
use crate::common::{Args, RecipeData};
use crate::{
    common::{config, logs::ShowLogsOnDrop, nrql},
    linux::{self, install::install_agent_control_from_recipe},
};
use std::time::Duration;
use tracing::info;

pub fn test_installation_with_infra_agent(args: Args) {
    let recipe_data = RecipeData {
        args,
        monitoring_source: "infra-agent".to_string(),
        ..Default::default()
    };
    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    info!("Setup Agent Control config");
    config::update_config_for_debug_logging(linux::DEFAULT_CONFIG_PATH, linux::DEFAULT_LOG_PATH);
    config::update_config_for_host_id(linux::DEFAULT_CONFIG_PATH, &test_id);

    linux::service::restart_service(linux::SERVICE_NAME);
    let _show_logs = ShowLogsOnDrop::from(linux::DEFAULT_LOG_PATH);

    let nrql_query = format!(r#"SELECT * FROM SystemSample WHERE `host.id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });
}
