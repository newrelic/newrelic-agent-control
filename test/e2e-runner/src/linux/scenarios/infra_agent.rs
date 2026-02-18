use crate::common::config::write_agent_local_config;
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{Args, RecipeData};
use crate::{
    common::{config, nrql},
    linux::{
        self,
        install::{install_agent_control_from_recipe, tear_down_test},
    },
};
use std::time::Duration;
use tracing::info;

pub fn test_installation_with_infra_agent(args: Args) {
    let recipe_data = RecipeData {
        args,
        monitoring_source: "infra-agent".to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    info!("Setup Agent Control config");
    let debug_log_config = config::ac_debug_logging_config(linux::DEFAULT_LOG_PATH);
    let config = format!(
        r#"
host_id: {test_id}
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
{debug_log_config}
"#
    );
    config::update_config(linux::DEFAULT_AC_CONFIG_PATH, config);

    write_agent_local_config(
        &linux::local_config_path("nr-infra"),
        String::from(
            r#"
config_agent:
  status_server_enabled: true
  status_server_port: 18003
  license_key: '{{NEW_RELIC_LICENSE_KEY}}'
    "#,
        ),
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    let nrql_query = format!(r#"SELECT * FROM SystemSample WHERE `host.id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });
}
