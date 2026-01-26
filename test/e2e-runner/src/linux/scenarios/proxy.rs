use crate::common::{Args, RecipeData};
use crate::{
    common::{config, logs::ShowLogsOnDrop, nrql, test::retry},
    linux::{self, bash::exec_bash_command, install::install_agent_control_from_recipe},
};
use std::time::Duration;
use tracing::{debug, info};

const PROXY_CONTAINER_NAME: &str = "mitmproxy-e2e";
const PROXY_URL: &str = "http://localhost:8080";
const PROXY_CA_DIR: &str = "/tmp/mitm-ca";
const PROXY_CA_CERT: &str = "/tmp/mitm-ca/mitmproxy-ca-cert.pem";

/// ac-e2e-host-no-deployment fleet on canaries account
const FLEET_ID: &str = "NjQyNTg2NXxOR0VQfEZMRUVUfDAxOWE5NjY2LTkxYzQtN2M0My1hNzZhLWY0YWVmODE4NWM4NA";

/// Domains expected to be reached through proxy
const EXPECTED_DOMAINS: &[&str] = &[
    // Installation
    "download.newrelic.com",
    // Keys generation and Agent Control authentication
    "publickeys.newrelic.com",
    "system-identity-oauth.service.newrelic.com",
    "identity-api.newrelic.com",
    // Agent Control OpAMP requests
    "opamp.service.newrelic.com",
    // Infra-Agent connections
    "infra-api.newrelic.com",
];

pub fn test_agent_control_proxy(args: Args) {
    info!("Setting up mitmproxy container");
    setup_mitmproxy();

    info!("Installing Agent Control with proxy configuration");
    let recipe_data = RecipeData {
        args,
        monitoring_source: "infra-agent".to_string(),
        proxy_url: PROXY_URL.to_string(),
        fleet_enabled: "true".to_string(),
        fleet_id: FLEET_ID.to_string(),
        ..Default::default()
    };
    install_agent_control_from_recipe(&recipe_data);

    let test_id = format!(
        "onhost-e2e-proxy_{}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );

    info!("Setup Agent Control config with proxy");
    config::update_config_for_debug_logging(linux::DEFAULT_CONFIG_PATH, linux::DEFAULT_LOG_PATH);
    config::update_config_for_host_id(linux::DEFAULT_CONFIG_PATH, &test_id);

    linux::service::restart_service(linux::SERVICE_NAME);
    let _show_logs = ShowLogsOnDrop::from(linux::DEFAULT_LOG_PATH);

    info!("Verifying that agent is reporting data through proxy");
    let nrql_query = format!(r#"SELECT * FROM SystemSample WHERE `host.id` = '{test_id}' LIMIT 1"#);
    info!(nrql = nrql_query, "Checking results of NRQL");
    let retries = 60;
    retry(retries, Duration::from_secs(10), "nrql assertion", || {
        nrql::check_query_results_are_not_empty(&recipe_data.args, &nrql_query)
    })
    .unwrap_or_else(|err| {
        panic!("query '{nrql_query}' failed after {retries} retries: {err}");
    });

    info!(
        expected_domains = EXPECTED_DOMAINS.join(", "),
        "Verifying proxy was used as expected by checking mitmproxy logs"
    );
    verify_proxy_usage();
}

fn setup_mitmproxy() {
    // Create directory for mitmproxy CA certificates
    info!("Creating directory for mitmproxy CA certificates");
    exec_bash_command(&format!("mkdir -p {}", PROXY_CA_DIR))
        .unwrap_or_else(|err| panic!("Failed to create CA directory: {err}"));

    // Clean up any existing mitmproxy container
    let cleanup_cmd = format!("docker rm -f {} 2>/dev/null || true", PROXY_CONTAINER_NAME);
    exec_bash_command(&cleanup_cmd)
        .unwrap_or_else(|err| panic!("Failed cleaning up previous mitmproxy containers: {err}"));

    // Start mitmproxy container.
    // We can use 8081 port to access mitproxy web interface and inspect traffic
    // `-it` is needed for mitmweb to show logs. See <https://github.com/mitmproxy/mitmproxy/issues/5727>
    let docker_cmd = format!(
        r#"docker run -d -it --rm --name {PROXY_CONTAINER_NAME} \
           -p 8080:8080 \
           -p 8081:8081 \
           -v {PROXY_CA_DIR}:/home/mitmproxy/.mitmproxy/ \
           mitmproxy/mitmproxy:12 \
           mitmweb --web-host 0.0.0.0"#, // Add `--set web_password=some-password` to set a fixed password for web ui
    );
    info!("Starting mitmproxy container");
    debug!("Command:\n{docker_cmd}");

    exec_bash_command(&docker_cmd)
        .unwrap_or_else(|err| panic!("Failed to start mitmproxy container: {err}"));

    // Wait for mitmproxy to generate certificates
    info!("Waiting for mitmproxy to generate certificates");
    let retries = 30;
    retry(
        retries,
        Duration::from_secs(2),
        "wait for proxy cert",
        || exec_bash_command(&format!("test -f {}", PROXY_CA_CERT)),
    )
    .unwrap_or_else(|err| {
        panic!("Proxy certificate not generated after {retries} retries: {err}");
    });

    // Add proxy CA certificate to system trust store
    info!("Adding proxy CA certificate to system trust store");
    let add_cert_cmd = format!(
        r#"
        cp {} /usr/local/share/ca-certificates/mitmproxy-ca-cert.crt && \
        update-ca-certificates
        "#,
        PROXY_CA_CERT
    );
    exec_bash_command(&add_cert_cmd)
        .unwrap_or_else(|err| panic!("Failed to add CA certificate to trust store: {err}"));

    // Verify proxy is working
    info!("Verifying proxy is working");
    let test_proxy_cmd =
        format!("curl --max-time 5 --proxy {PROXY_URL} -I -L https://newrelic.com/",);
    retry(10, Duration::from_secs(2), "test proxy connection", || {
        exec_bash_command(&test_proxy_cmd)
    })
    .unwrap_or_else(|err| {
        panic!("Proxy is not working correctly: {err}");
    });

    info!("Mitmproxy setup completed successfully");
}

/// Checks the mitmproxy logs to check if expected domains where reached
fn verify_proxy_usage() {
    let check_logs_cmd = format!(
        "docker logs {} 2>&1 | grep -i 'newrelic' || echo 'No New Relic traffic found'",
        PROXY_CONTAINER_NAME
    );

    let logs = exec_bash_command(&check_logs_cmd)
        .unwrap_or_else(|err| panic!("Failed to check proxy logs: {err}"));

    for domain in EXPECTED_DOMAINS {
        if !logs.contains(domain) {
            info!(logs = %logs, "Mitmproxy logs");
            panic!("No connection to '{domain}' found in Mitmproxy logs");
        }
    }
    debug!(logs = %logs, "Mitmproxy logs");
}
