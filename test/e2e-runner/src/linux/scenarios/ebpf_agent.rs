use crate::common::InstallationArgs;
use crate::common::RecipeData;
use crate::common::config::write_agent_local_config;
use crate::common::nrql::Region;
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::{
    common::{config, nrql},
    linux::{
        self,
        install::{install_agent_control_from_recipe, tear_down_test},
    },
};
use config::DEBUG_LOGGING_CONFIG;
use std::time::Duration;
use tracing::info;

pub fn test_ebpf_agent(args: InstallationArgs) {
    let infra_version = args
        .infra_agent_version
        .clone()
        .expect("--infra-agent-version is required for this scenario");

    let staging = args.nr_region == Region::Staging;

    let recipe_data = RecipeData {
        args,
        monitoring_source: "infra-agent".to_string(),
        recipe_list: "agent-control,ebpf-agent-installer".to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    info!("Setup Agent Control config with eBPF");
    let config = format!(
        r#"
host_id: {test_id}
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
  nr-ebpf:
    agent_type: "newrelic/com.newrelic.ebpf:0.1.0"
{DEBUG_LOGGING_CONFIG}
"#
    );
    config::update_config(linux::DEFAULT_AC_CONFIG_PATH, config);
    // eBPF agent config
    let ebpf_config = if staging {
        format!(
            r#"
config_agent:
  DEPLOYMENT_NAME: {test_id}
  OTLP_ENDPOINT: staging-otlp.nr-data.net:443
    "#
        )
    } else {
        format!(
            r#"
config_agent:
  DEPLOYMENT_NAME: {test_id}
    "#
        )
    };
    write_agent_local_config(&linux::local_config_path("nr-ebpf"), ebpf_config);
    // Infra agent config: it is used to generate traffic for eBPF metrics to appear
    write_agent_local_config(
        &linux::local_config_path("nr-infra"),
        format!(
            r#"
config_agent:
  license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
  staging: {staging}
version: {}
"#,
            infra_version
        ),
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    let nrql_query = format!(
        r#"SELECT * FROM Metric WHERE metricName = 'ebpf.tcp.connection_duration' AND deployment.name = '{test_id}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });
}
