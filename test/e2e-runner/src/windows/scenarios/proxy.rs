use crate::common::config::{ac_debug_logging_config, update_config, write_agent_local_config};
use crate::common::exec::LongRunningProcess;
use crate::common::nrql::check_query_results_are_not_empty;
use crate::common::on_drop::CleanUp;
use crate::common::test::retry_panic;
use crate::common::{Args, RecipeData};
use crate::windows::install::{SERVICE_NAME, install_agent_control_from_recipe, tear_down_test};
use crate::windows::powershell::{download_file, exec_ps, extract};
use crate::windows::scenarios::INFRA_AGENT_VERSION;
use crate::windows::service::{STATUS_RUNNING, check_service_status};
use crate::windows::utils::as_user_dir;
use crate::windows::{self};
use std::process::Command;
use std::time::Duration;
use tracing::info;

const MITMPROXY_VERSION: &str = "12.2.1";
const PROXY_URL: &str = "http://localhost:8080";
const MITMPROXY_DIR: &str = "\\mitmproxy";
const MITMPROXY_CA_CERT: &str = "\\.mitmproxy\\mitmproxy-ca-cert.cer";
const MITMPROXY_BIN: &str = "\\mitmproxy\\mitmdump.exe";
const MITMPROXY_ZIP: &str = "\\mitmproxy.zip";

/// Domains expected to be reached through proxy
const EXPECTED_DOMAINS: &[&str] = &[
    // Installation
    "download.newrelic.com",
    // Keys generation and Agent Control authentication
    "publickeys.newrelic.com",
    "system-identity-oauth.service.newrelic.com",
    // Agent Control OpAMP requests
    "opamp.service.newrelic.com",
    // Infra-Agent connections
    "infra-api.newrelic.com",
    "identity-api.newrelic.com",
];

/// ac-e2e-host-no-deployment fleet on canaries account
const FLEET_ID: &str = "NjQyNTg2NXxOR0VQfEZMRUVUfDAxOWE5NjY2LTkxYzQtN2M0My1hNzZhLWY0YWVmODE4NWM4NA";

/// Installs AC configured to use a proxy and verifies that the proxy is used.
pub fn test_proxy(args: Args) {
    info!("Setting up proxy");
    let mitm_process = setup_mitmproxy();

    info!("Installing Agent Control with proxy configuration");
    let recipe_data = RecipeData {
        args,
        proxy_url: PROXY_URL.to_string(),
        fleet_enabled: "true".to_string(),
        fleet_id: FLEET_ID.to_string(),
        ..Default::default()
    };

    let _clean_up = CleanUp::new(tear_down_test);

    install_agent_control_from_recipe(&recipe_data);
    let test_id = format!(
        "onhost-e2e-proxy_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    let debug_log_config = ac_debug_logging_config(windows::DEFAULT_LOG_PATH);

    // Install cli does not support adding infra-agent config yet on windows, so we need to update the config manually
    update_config(
        windows::DEFAULT_CONFIG_PATH,
        format!(
            r#"
host_id: {test_id}
agents:
  nr-infra:
    agent_type: newrelic/com.newrelic.infrastructure:0.1.0
{debug_log_config}
"#
        ),
    );

    write_agent_local_config(
        windows::DEFAULT_NR_INFRA_PATH,
        format!(
            r#"
config_agent:
  proxy: {PROXY_URL}
  license_key: '{{{{NEW_RELIC_LICENSE_KEY}}}}'
version: {INFRA_AGENT_VERSION}
"#
        ),
    );

    windows::service::restart_service(SERVICE_NAME);

    retry_panic(
        10,
        Duration::from_secs(2),
        "checking ac service is started",
        || check_service_status(SERVICE_NAME, STATUS_RUNNING),
    );

    let nrql_query = format!(r#"SELECT * FROM SystemSample WHERE `host.id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Checking results of NRQL");
    retry_panic(60, Duration::from_secs(10), "nrql assertion", || {
        check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    });

    info!("Verifying proxy was used as expected by checking mitmproxy logs");
    let logs = mitm_process.current_output();
    for domain in EXPECTED_DOMAINS {
        if !logs.contains(domain) {
            info!(logs = %logs, "Mitmproxy logs");
            panic!("No connection to '{domain}' found in Mitmproxy logs");
        }
    }

    info!("Test completed successfully");
}

/// Downloads, starts and add mitmproxy CA certificate to system trust store.
fn setup_mitmproxy() -> LongRunningProcess {
    info!("Downloading mitmproxy");
    download_file(
        format!(
            "https://downloads.mitmproxy.org/{}/mitmproxy-{}-windows-x86_64.zip",
            MITMPROXY_VERSION, MITMPROXY_VERSION
        ),
        as_user_dir(MITMPROXY_ZIP),
    );

    info!("Extracting mitmproxy");
    extract(as_user_dir(MITMPROXY_ZIP), as_user_dir(MITMPROXY_DIR));

    info!("Starting mitmproxy");
    let mut cmd = Command::new(as_user_dir(MITMPROXY_BIN));
    cmd.args([
        "--listen-host",
        "0.0.0.0",
        "--listen-port",
        "8080",
        // print high level flow details to stdout for verification
        "--flow-detail=1",
    ]);

    info!("Spawning process: {:?}", cmd);
    let mitm_process = LongRunningProcess::spawn(cmd);

    info!("Waiting for mitmproxy to start");
    retry_panic(30, Duration::from_secs(2), "waiting proxy to start", || {
        let output = mitm_process.current_output();
        if output.contains("proxy listening at") {
            Ok(())
        } else {
            Err(format!("mitmproxy not started yet. Current output: {}", output).into())
        }
    });

    info!("Adding mitmproxy CA certificate to system trust store");
    exec_ps(format!(
        "Import-Certificate -FilePath '{}' -CertStoreLocation Cert:\\LocalMachine\\Root",
        as_user_dir(MITMPROXY_CA_CERT)
    ))
    .unwrap_or_else(|err| panic!("Failed to import mitmproxy CA certificate: {}", err));

    info!("Verifying proxy is working");
    let result = exec_ps(format!(
        "Invoke-WebRequest -Uri 'https://www.newrelic.com' -UseBasicParsing -Proxy '{}' | Select-Object -ExpandProperty StatusCode",
        PROXY_URL
    ))
        .unwrap_or_else(|err| panic!("Failed to verify proxy is working: {}", err));

    if !result.contains("200") {
        panic!(
            "Proxy verification failed: expected status code 200, got: {}",
            result
        );
    }

    info!("Mitmproxy setup completed successfully");
    mitm_process
}
