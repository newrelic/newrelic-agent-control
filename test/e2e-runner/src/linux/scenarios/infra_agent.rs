use crate::common::config::{DEBUG_LOGGING_CONFIG, update_config, write_agent_local_config};
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{Args, RecipeData};
use crate::{
    common::nrql,
    linux::{
        self,
        install::{install_agent_control_from_recipe, tear_down_test},
    },
};
use std::time::Duration;
use tracing::info;

pub fn test_installation_with_infra_agent(args: Args) {
    let infra_version = args
        .infra_agent_version
        .clone()
        .expect("--infra-agent-version is required for this scenario");

    let recipe_data = RecipeData {
        args,
        monitoring_source: "infra-agent".to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    let infra_agent_id: &str = "nr-infra";

    info!("Setup Agent Control config");
    update_config(
        linux::DEFAULT_AC_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
{DEBUG_LOGGING_CONFIG}
"#
        ),
    );

    write_agent_local_config(
        &linux::local_config_path(infra_agent_id),
        format!(
            r#"
config_agent:
  license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
  log:
    level: debug
config_logging:
    logging.yml:
      logs:
      - name: syslog
        file: /var/log/syslog
        attributes:
          host.id: {test_id}
version: {}
"#,
            infra_version
        ),
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    let nrql_query = format!(r#"SELECT * FROM SystemSample WHERE `host.id` = '{test_id}' LIMIT 1"#);
    info!(
        nrql = nrql_query,
        "Checking results of NRQL to check SystemSample"
    );
    let retries = 60;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    let nrql_query = format!(r#"SELECT * FROM Log WHERE `host.id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Checking results of NRQL to check logs");
    let retries = 30;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    info!("Test completed successfully");
}
