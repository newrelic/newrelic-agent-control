use std::{thread, time::Duration};

use tracing::info;

use crate::{
    linux::{self, install::RecipeInstallationData},
    tools::{
        config,
        logs::show_logs,
        nrql,
        test::{TestResult, retry},
    },
};

pub fn test_installation_with_infra_agent() -> TestResult<()> {
    // TODO: input / env-var args
    let recipe_data = RecipeInstallationData::default();

    info!("Install Agent Control");
    linux::install::install_agent_control_from_recipe(recipe_data)?;

    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    info!("Setup Agent Control config");
    let debug_log_config = config::debug_logging_config(linux::DEFAULT_CONFIG_PATH);
    let config = format!(
        r#"
host_id: {test_id}
{debug_log_config}
"#
    );
    config::update_config(linux::DEFAULT_CONFIG_PATH, &config)?;

    thread::sleep(Duration::from_secs(5));

    linux::service::restart_service(linux::SERVICE_NAME)?;

    // TODO: PROPER VALUES
    let api_endpoint = "";
    let api_key = "";
    let account_id = 0;
    let nrql_query = format!(r#"SELECT * FROM SystemSample WHERE `host.id` = {test_id} LIMIT 1"#);
    retry(120, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(api_endpoint, api_key, account_id, &nrql_query)
    })?;

    show_logs(linux::DEFAULT_LOG_PATH)?;

    Ok(())
}
