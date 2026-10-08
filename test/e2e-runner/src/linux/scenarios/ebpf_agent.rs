use crate::common::InstallationArgs;
use crate::common::RecipeData;
use crate::common::config::write_agent_local_config;
use crate::common::nrql::Region;
use crate::common::on_drop::CleanUp;
use crate::common::test::retry;
use crate::{
    common::{config, nrql},
    linux::{
        self,
        bash::exec_bash_command,
        install::{install_latest_agent_control, tear_down_test},
    },
};
use config::DEBUG_LOGGING_CONFIG;
use std::time::Duration;
use tracing::info;

pub fn test_ebpf_agent(args: InstallationArgs) {
    let infra_agent_version = args
        .infra_agent_version
        .clone()
        .expect("--infra-agent-version is required for this scenario");

    let ebpf_version = args
        .ebpf_agent_version
        .clone()
        .expect("--ebpf-agent-version is required for this scenario");

    let staging = args.nr_region == Region::Staging;

    let recipe_data = RecipeData {
        args,
        recipe_list: "agent-control".to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_latest_agent_control(&recipe_data);

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
    let region = if staging { "staging" } else { "US" };
    let ebpf_config = format!(
        r#"
config:
  deploymentName: "{test_id}"
  region: "{region}"
  logLevel: "DEBUG"
enable_file_logging: true
version: "{ebpf_version}"
    "#
    );
    write_agent_local_config(&linux::local_config_path("nr-ebpf"), ebpf_config);
    // Infra agent config: it is used to generate traffic for eBPF metrics to appear
    write_agent_local_config(
        &linux::local_config_path("nr-infra"),
        format!(
            r#"
config_agent:
  license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
  staging: {staging}
version: {infra_agent_version}
"#
        ),
    );

    linux::service::restart_service(linux::SERVICE_NAME);

    let nrql_query = format!(
        r#"SELECT * FROM Metric WHERE metricName = 'ebpf.tcp.connection_duration' AND deployment.name = '{test_id}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    let result = retry(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    dump_ebpf_logs();

    if let Err(err) = result {
        panic!("Operation 'nrql assertion' failed after {retries} retries: {err}");
    }
}

fn dump_ebpf_logs() {
    let service_logs = exec_bash_command("cat /var/log/newrelic-agent-control/nr-ebpf/*")
        .expect("logs must be there");
    println!(
        "--- eBPF agent service logs start---\n{service_logs}\n--- eBPF agent service logs end ---"
    );

    let status_logs =
        exec_bash_command("cat /var/lib/newrelic-agent-control/filesystem/nr-ebpf/logs/*")
            .expect("logs must be there");
    println!(
        "--- eBPF agent status logs start---\n{status_logs}\n--- eBPF agent status logs end ---"
    );
}
