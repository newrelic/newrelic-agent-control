use std::time::Duration;

use tracing::info;

use crate::{
    linux::{
        self,
        install::{Args, RecipeData, install_agent_control_from_recipe},
    },
    tools::{config, logs::ShowLogsOnDrop, nrql, test::retry},
};

pub fn test_ebpf_agent(args: Args) {
    let recipe_data = RecipeData {
        args,
        monitoring_source: "infra-agent".to_string(),
        recipe_list: "agent-control,ebpf-agent-installer".to_string(),
        ..Default::default()
    };
    install_agent_control_from_recipe(&recipe_data);
    let test_id = format!(
        "onhost-e2e-infra-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    info!("Setup Agent Control config with eBPF");
    let debug_log_config = config::debug_logging_config(linux::DEFAULT_LOG_PATH);
    let config = format!(
        r#"
host_id: {test_id}
agents:
  nr-infra:
    agent_type: "newrelic/com.newrelic.infrastructure:0.1.0"
  nr-ebpf:
    agent_type: "newrelic/com.newrelic.ebpf:0.1.0"
{debug_log_config}
"#
    );
    config::update_config(linux::DEFAULT_CONFIG_PATH, &config);
    // eBPF agent config
    config::write_agent_local_config(
        "/etc/newrelic-agent-control/local-data/nr-ebpf",
        format!(
            r#"
config_agent:
  DEPLOYMENT_NAME: {test_id}
    "#
        ),
    );
    // Infra agent config: it is used to generate traffic for eBPF metrics to appear
    config::write_agent_local_config(
        "/etc/newrelic-agent-control/local-data/nr-infra",
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
    let _show_logs = ShowLogsOnDrop::from(linux::DEFAULT_LOG_PATH);

    let nrql_query = format!(
        r#"SELECT * FROM Metric WHERE metricName = 'ebpf.tcp.connection_duration' AND deployment.name = '{test_id}' LIMIT 1"#
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
