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
        bash::exec_bash_command,
        install::{install_agent_control_from_recipe, tear_down_test},
    },
};
use config::DEBUG_LOGGING_CONFIG;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tracing::info;

// Regression check for the eBPF agent's own status-log write: it must land under the AC managed
// filesystem directory (via NEW_RELIC_LOG_FILE_PATH), not fall back to its hardcoded default.
const EBPF_STATUS_LOG: &str =
    "/var/lib/newrelic-agent-control/filesystem/nr-ebpf/logs/ebpf-agent-status.log";
const EBPF_DEFAULT_STATUS_LOG: &str = "/etc/newrelic-ebpf-agent/ebpf-agent-status.log";

// Plain HTTP traffic for the eBPF agent to trace.
const HTTP_SERVER_PORT: u16 = 18080;

pub fn test_ebpf_agent(args: InstallationArgs) {
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
        "onhost-e2e-ebpf-agent_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
    );

    info!("Setup Agent Control config with eBPF");
    let config = format!(
        r#"
host_id: {test_id}
agents:
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
version: "{ebpf_version}"
    "#
    );
    write_agent_local_config(&linux::local_config_path("nr-ebpf"), ebpf_config);

    linux::service::restart_service(linux::SERVICE_NAME);

    let _http_traffic = HttpTraffic::start();

    let nrql_query = format!(
        r#"SELECT * FROM Metric WHERE metricName = 'ebpf.tcp.connection_duration' AND deployment.name = '{test_id}' LIMIT 1"#
    );
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry_panic(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    info!(
        path = EBPF_STATUS_LOG,
        "Asserting the eBPF agent's status log is under the AC managed filesystem directory"
    );
    // The agent's own snapshot timer only fires every 120s, so the file may not exist yet.
    retry_panic(
        15,
        Duration::from_secs(10),
        "ebpf status log written under the AC managed filesystem directory",
        || exec_bash_command(&format!("test -f '{EBPF_STATUS_LOG}'")),
    );
    exec_bash_command(&format!("test ! -f '{EBPF_DEFAULT_STATUS_LOG}'"))
        .expect("eBPF status log should not fall back to /etc/newrelic-ebpf-agent");
}

/// Local HTTP server plus a client loop hitting it, both killed on drop.
struct HttpTraffic {
    server: Child,
    client: Child,
}

impl HttpTraffic {
    fn start() -> Self {
        info!(port = HTTP_SERVER_PORT, "Starting HTTP traffic generator");
        let port = HTTP_SERVER_PORT.to_string();
        let server = Command::new("python3")
            .args(["-m", "http.server", &port, "--bind", "127.0.0.1"])
            .current_dir("/tmp")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("HTTP server should start");
        let client = Command::new("bash")
            .args([
                "-c",
                &format!(
                    "while true; do curl -s -o /dev/null http://127.0.0.1:{port}/; sleep 2; done"
                ),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("HTTP client loop should start");
        Self { server, client }
    }
}

impl Drop for HttpTraffic {
    fn drop(&mut self) {
        for child in [&mut self.client, &mut self.server] {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
