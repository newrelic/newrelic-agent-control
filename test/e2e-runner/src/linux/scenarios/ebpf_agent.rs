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
        install::{install_agent_control_from_recipe, tear_down_test},
    },
};
use config::DEBUG_LOGGING_CONFIG;
use std::time::Duration;
use tracing::info;

// Regression check for the eBPF agent's own status-log write: it must land under the AC managed
// filesystem directory (via NEW_RELIC_LOG_FILE_PATH), not fall back to its hardcoded default.
const EBPF_STATUS_LOG: &str =
    "/var/lib/newrelic-agent-control/filesystem/nr-ebpf/logs/ebpf-agent-status.log";
const EBPF_DEFAULT_STATUS_LOG: &str = "/etc/newrelic-ebpf-agent/ebpf-agent-status.log";

// Plain HTTP traffic for the eBPF agent to trace. The server must never do DNS before serving
// HTTP: the eBPF agent caches a process's entity from the first protocol it sees, and a DNS-first
// entity gets no name, so all of its data (TCP stats included) is dropped.
const HTTP_SERVER_UNIT: &str = "ebpf-e2e-http-server";
const HTTP_CLIENT_UNIT: &str = "ebpf-e2e-http-client";
const HTTP_SERVER_PORT: u16 = 18080;

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
    let region = if staging { "staging" } else { "US" };
    let ebpf_config = format!(
        r#"
config:
  deploymentName: "{test_id}"
  region: "{region}"
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

    let _stop_http_traffic = CleanUp::new(stop_http_traffic);
    start_http_traffic();

    exec_bash_command(&format!("test ! -f '{EBPF_DEFAULT_STATUS_LOG}'"))
        .expect("eBPF status log should not fall back to /etc/newrelic-ebpf-agent");

    let nrql_query = format!(
        r#"SELECT * FROM Metric WHERE metricName = 'ebpf.tcp.connection_duration' AND deployment.name = '{test_id}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    if let Err(err) = retry(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    }) {
        dump_ebpf_logs();
        panic!("Operation 'nrql assertion' failed after {retries} retries: {err}");
    }

    info!(
        path = EBPF_STATUS_LOG,
        "Asserting the eBPF agent's status log is under the AC managed filesystem directory"
    );
    // The agent's own snapshot timer only fires every 120s, so the file may not exist yet.
    let retries = 15;
    if let Err(err) = retry(
        retries,
        Duration::from_secs(10),
        "ebpf status log written under the AC managed filesystem directory",
        || exec_bash_command(&format!("test -f '{EBPF_STATUS_LOG}'")),
    ) {
        dump_ebpf_logs();
        panic!(
            "Operation 'ebpf status log written under the AC managed filesystem directory' failed after {retries} retries: {err}"
        );
    }

    dump_ebpf_logs();
}

fn start_http_traffic() {
    info!(port = HTTP_SERVER_PORT, "Starting HTTP traffic generator");
    exec_bash_command(&format!(
        "systemd-run --unit {HTTP_SERVER_UNIT} --working-directory /tmp \
         python3 -m http.server {HTTP_SERVER_PORT} --bind 127.0.0.1"
    ))
    .expect("HTTP server should start");
    exec_bash_command(&format!(
        "systemd-run --unit {HTTP_CLIENT_UNIT} bash -c \
         'while true; do curl -s -o /dev/null http://127.0.0.1:{HTTP_SERVER_PORT}/; sleep 2; done'"
    ))
    .expect("HTTP client loop should start");
}

fn stop_http_traffic() {
    let _ = exec_bash_command(&format!(
        "systemctl stop {HTTP_CLIENT_UNIT} {HTTP_SERVER_UNIT}"
    ));
}

fn dump_ebpf_logs() {
    let service_logs = exec_bash_command("cat /var/log/newrelic-agent-control/nr-ebpf/*")
        .expect("logs must be there");
    println!("eBPF agent service logs:\n{service_logs}");

    let status_logs =
        exec_bash_command("cat /var/lib/newrelic-agent-control/filesystem/nr-ebpf/logs/*")
            .expect("logs must be there");
    println!("eBPF agent status logs:\n{status_logs}");
}
